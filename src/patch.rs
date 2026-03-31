use patchouly_core::{
    Stencil, StencilFamily,
    relocation::{DelayedRelocation, JumpTarget},
    stencils::Variable,
};
use smallvec::SmallVec;

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
