use patchouly_core::{
    Stencil, StencilFamily,
    relocation::{DelayedRelocation, JumpTarget},
    stencils::Location,
};
use smallvec::SmallVec;
use std::mem::size_of;

pub struct PatchArgs<'a, const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
    pub &'a [Location; IN],
    pub &'a [Location; OUT],
    pub &'a [usize; HOLES],
    pub &'a [JumpTarget; JUMPS],
    pub bool,
);
pub trait CopyNPatch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize> {
    fn copy(&self, store: &[u8], dest: &mut Vec<u8>);
    fn patch<const MAX_REGS: usize>(
        &self,
        store: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        args: PatchArgs<IN, OUT, HOLES, JUMPS>,
        dest: &mut Vec<u8>,
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
        PatchArgs(inputs, outputs, holes, jumps, wide): PatchArgs<IN, OUT, HOLES, JUMPS>,
        dest: &mut Vec<u8>,
        offset: usize,
        relocations: &mut Vec<DelayedRelocation>,
    ) {
        let mut stack_vars = SmallVec::<[usize; 8]>::new();
        for var in inputs.iter().chain(outputs.iter()) {
            if let Location::Stack(i) = var {
                stack_vars.push(*i as usize);
            }
        }

        let mut holes = *holes;
        if wide {
            dest.resize(dest.len().next_multiple_of(size_of::<usize>()), 0);
            for hole in holes.as_mut() {
                let addr = dest.len();
                dest.extend_from_slice(hole.to_ne_bytes().as_ref());
                *hole = addr;
            }
        }

        for relocation in self.relocations(store) {
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
