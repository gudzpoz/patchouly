use patchouly_core::{
    Stencil, StencilFamily,
    relocation::{PatchInfo, Relocation, RelocationEncoding},
    stencils::Variable,
};
use smallvec::SmallVec;

#[derive(Copy, Clone)]
pub enum JumpTarget {
    /// To the next stencil
    Next,
    /// TODO: To a specific block id
    Target(u16),
}

pub struct DelayedRelocation {
    offset: usize,
    relocation: Relocation,
    target: u16,
}
impl DelayedRelocation {
    pub fn try_apply(
        dest: &mut [u8],
        offset: usize,
        relocation: Relocation,
        stack_vars: &[usize],
        holes: &[usize],
        jumps: &[JumpTarget],
    ) -> Option<Self> {
        let value = match relocation.patch_info().unwrap() {
            // TODO: unwrap safety?
            PatchInfo::Hole(i) => holes[i as usize],
            PatchInfo::Stack(i) => stack_vars[i as usize],
            PatchInfo::Target(i) => match jumps[i as usize] {
                // jump to the end
                JumpTarget::Next => dest.len() - offset - relocation.offset() as usize,
                // delayed
                JumpTarget::Target(target) => {
                    return Some(DelayedRelocation {
                        offset,
                        relocation,
                        target,
                    });
                }
            },
        };

        apply_raw(&mut dest[offset..], value, relocation);
        None
    }

    pub fn target(&self) -> u16 {
        self.target
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn apply(&self, dest: &mut [u8], value: isize) {
        apply_raw(&mut dest[self.offset..], value as usize, self.relocation);
    }
}

pub struct PatchArgs<'a, const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
    pub &'a [Variable; IN],
    pub &'a [Variable; OUT],
    pub &'a [usize; HOLES],
    pub &'a [JumpTarget; JUMPS],
);
pub trait CopyNPatch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize> {
    fn copy(&self, store: &[u8], dest: &mut Vec<u8>);
    fn patch<const MAX_REGS: usize>(
        &self,
        store: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        args: PatchArgs<IN, OUT, HOLES, JUMPS>,
        dest: &mut [u8],
        offset: usize,
        relocations: &mut Vec<DelayedRelocation>,
    );
}
impl<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>
    CopyNPatch<IN, OUT, HOLES, JUMPS> for Stencil<IN, OUT, HOLES, JUMPS>
{
    fn copy(&self, store: &[u8], dest: &mut Vec<u8>) {
        dest.extend_from_slice(self.code(store));
    }

    fn patch<const MAX_REGS: usize>(
        &self,
        store: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        PatchArgs(inputs, outputs, holes, jumps): PatchArgs<IN, OUT, HOLES, JUMPS>,
        dest: &mut [u8],
        offset: usize,
        relocations: &mut Vec<DelayedRelocation>,
    ) {
        let mut stack_vars = SmallVec::<[usize; 8]>::new();
        for var in inputs.iter().chain(outputs.iter()) {
            if let Variable::Stack(i) = var {
                stack_vars.push(*i as usize);
            }
        }

        for relocation in &store.relocation_data[self.relocation_index as usize..] {
            if relocation.is_invalid() {
                return;
            }
            let delayed = DelayedRelocation::try_apply(
                dest,
                offset,
                *relocation,
                &stack_vars,
                holes.as_slice(),
                jumps.as_slice(),
            );
            if let Some(delayed) = delayed {
                relocations.push(delayed);
            }
        }
    }
}

fn apply_raw(dest: &mut [u8], value: usize, relocation: Relocation) {
    let value = value.wrapping_add_signed(relocation.addend() as isize);
    let offset = relocation.offset();
    let size = relocation.size();
    match relocation.encoding() {
        RelocationEncoding::Generic => {
            let size = (size / 8) as usize;
            dest[offset as usize..][..size].copy_from_slice(&value.to_le_bytes()[..size]);
        }
        RelocationEncoding::X86Signed => {
            let size = (size / 8) as usize;
            dest[offset as usize..][..size].copy_from_slice(&value.to_le_bytes()[..size]);
        }
        _ => unreachable!(),
    }
}
