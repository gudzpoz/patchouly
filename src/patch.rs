use memmap2::MmapMut;
use patchouly_core::{
    Stencil, StencilFamily, StencilLibrary,
    relocation::{DelayedRelocation, DelayedTarget, JumpTarget, PatchInfo},
    stencils::{Location, SelectedStencil},
};
use smallvec::SmallVec;

use crate::{PatchError, Program};

#[derive(Default)]
pub struct ProgramBlocks {
    pub offsets: Vec<usize>,
}
impl ProgramBlocks {
    pub fn from_lens(mut lens: Vec<usize>) -> (Self, usize) {
        let len = if let Some(mut last) = lens.first().cloned() {
            lens[0] = 0;
            for i in lens.iter_mut().skip(1) {
                let next = last + *i;
                *i = last;
                last = next;
            }
            last
        } else {
            0
        };
        (Self { offsets: lens }, len)
    }

    fn resolve_target(&self, block: u16) -> Option<usize> {
        self.offsets.get(block as usize).copied()
    }
}

#[derive(Clone)]
pub struct PatchBlock<const MAX_REGS: usize> {
    library: &'static StencilLibrary<MAX_REGS>,
    code: Vec<u8>,
    relocations: Vec<DelayedRelocation>,
    constants: Vec<usize>,
    next_relocations: Vec<DelayedRelocation>,
    ended: bool,
}

struct PatchArgs<'a, const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
    &'a [Location; IN],
    &'a [Location; OUT],
    &'a [usize; HOLES],
    &'a [JumpTarget; JUMPS],
    bool,
);

impl<const MAX_REGS: usize> PatchBlock<MAX_REGS> {
    pub fn new(library: &'static StencilLibrary<MAX_REGS>) -> Self {
        Self {
            library,
            code: vec![],
            relocations: vec![],
            constants: vec![],
            next_relocations: vec![],
            ended: false,
        }
    }

    pub fn add<const IN: usize, const OUT: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, 1>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        if self.ended {
            return Err(PatchError::AlreadyEnded);
        }
        self.emit(stencil, inputs, outputs, holes, &[JumpTarget::Next])
    }

    pub fn ret<const IN: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, 0, MAX_REGS, HOLES, 0>,
        inputs: &[Location; IN],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        self.ended = true;
        self.emit(stencil, inputs, &[], holes, &[])
    }

    pub fn end_branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Result<(), PatchError> {
        self.ended = true;
        self.branch(stencil, inputs, outputs, holes, jumps)
    }

    pub fn branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Result<(), PatchError> {
        self.emit(stencil, inputs, outputs, holes, jumps)
    }

    /// Emits a stencil
    ///
    /// ## add/ret/branch
    ///
    /// The entry points, `add`, `ret`, and `branch` all delegate to `emit`.
    /// The main difference is that the latter two end the block.
    ///
    /// ## Loop
    ///
    /// An `emit` call does not yet emit valid code and requires:
    /// - `finalize` to patch in inter-block offsets and wide constants,
    /// - another `emit` call to prune useless jumps and calculate offsets.
    ///
    /// Basically, a stencil is processed as follows:
    ///
    /// - *this `emit` call*
    ///   - (handle previous stencil)
    ///   - copy code
    ///   - relocate inline constants
    /// - *next `emit` call*
    ///   - prune useless jumps at the end of the stencil
    ///   - relocate jumps jumping to the next stencil
    ///     (some stencils multiple jumps, all jumping to the next stencil;
    ///     but we can only know the offset after our pruning)
    ///   - (and then handle the next stencil)
    /// - ...
    /// - *finalize*
    ///   - relocate inter-block offsets
    ///   - store and relocate wide constants
    ///
    /// Note that this means that the last `emit` must not be a normal stencil that
    /// tries to jump to the next stencil.
    fn emit<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencils: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Result<(), PatchError> {
        if inputs.iter().any(|v| v.into_bits() as usize >= MAX_REGS)
            || outputs.iter().any(|v| v.into_bits() as usize >= MAX_REGS)
        {
            return Err(PatchError::InvalidRegister);
        }
        let Some(SelectedStencil { wide, stencil }) = stencils.select(inputs, outputs, holes)
        else {
            return Err(PatchError::StencilNotFound);
        };

        let next_target = self.code.len();
        if let Some(last) = self.next_relocations.pop() {
            last.apply(&mut self.code, last.resolve(0, next_target));
            if self.code.ends_with(self.library.empty)
                && last.location() > self.relocations.last().map(|r| r.location()).unwrap_or(0)
            {
                self.code
                    .truncate(self.code.len() - self.library.empty.len());
            }
        }
        let next_target = self.code.len();
        for relocation in self.next_relocations.drain(..) {
            relocation.apply(&mut self.code, relocation.resolve(0, next_target));
        }

        let from = self.code.len();
        self.copy_and_patch::<IN, OUT, HOLES, JUMPS>(
            stencils,
            stencil,
            PatchArgs(inputs, outputs, holes, jumps, wide),
            from,
        );

        Ok(())
    }

    fn copy_and_patch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        store: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        s: &Stencil<IN, OUT, HOLES, JUMPS>,
        PatchArgs(inputs, outputs, holes, jumps, wide): PatchArgs<IN, OUT, HOLES, JUMPS>,
        offset: usize,
    ) {
        self.code.extend_from_slice(s.code(self.library.code));

        let mut stack_vars = SmallVec::<[usize; 8]>::new();
        for var in inputs.iter().chain(outputs.iter()) {
            if let Location::Stack(i) = var {
                stack_vars.push(*i as usize);
            }
        }

        for relocation in s.relocations(store) {
            if relocation.is_invalid() {
                return;
            }
            if wide && let Some(PatchInfo::Hole(i)) = relocation.patch_info() {
                let constant_index = self.constants.len();
                self.constants.push(holes[i as usize]);
                self.relocations.push(DelayedRelocation::constant(
                    offset,
                    *relocation,
                    constant_index as u16,
                ));
                continue;
            }
            let delayed = DelayedRelocation::try_apply(
                &mut self.code,
                offset,
                *relocation,
                &stack_vars,
                holes,
                jumps.as_slice(),
            );
            if let Some(delayed) = delayed {
                if let DelayedTarget::Next = delayed.target() {
                    self.next_relocations.push(delayed);
                } else {
                    self.relocations.push(delayed);
                }
            }
        }
    }

    pub fn measure(&self) -> Option<usize> {
        if !self.ended {
            return None;
        }

        Some(self.final_len())
    }

    fn final_len(&self) -> usize {
        if self.constants.is_empty() {
            self.code.len()
        } else {
            self.code.len().next_multiple_of(size_of::<usize>())
                + self.constants.len() * size_of::<usize>()
        }
    }

    pub fn finalize_into(
        &self,
        map: &mut [u8],
        base: usize,
        block_base_offset: usize,
        blocks: &ProgramBlocks,
    ) -> Result<(), PatchError> {
        if !self.ended {
            return Err(PatchError::NotEnded);
        }
        if !self.next_relocations.is_empty() {
            // last stencil trying to jump over block end
            return Err(PatchError::InvalidEnd);
        }

        let code = &mut map[block_base_offset..];
        code[..self.code.len()].copy_from_slice(&self.code);
        let constant_base = if self.constants.is_empty() {
            code.len()
        } else {
            let base = self.code.len().next_multiple_of(size_of::<usize>());
            let mut start = base;
            for constant in &self.constants {
                let bytes = &constant.to_ne_bytes();
                let next = start + bytes.len();
                code[start..next].copy_from_slice(bytes);
                start = next;
            }
            base
        };

        let block_base = base + block_base_offset;
        for relocation in &self.relocations {
            let target = match relocation.target() {
                DelayedTarget::Block(block) => {
                    let target_offset = blocks
                        .resolve_target(block)
                        .ok_or(PatchError::UnresolvedBlockTarget)?;
                    base + target_offset
                }
                DelayedTarget::Constant(index) => {
                    block_base + constant_base + index as usize * size_of::<usize>()
                }
                DelayedTarget::Next => unreachable!(),
            };
            relocation.apply(code, relocation.resolve(block_base, target));
        }

        Ok(())
    }

    pub fn finalize(self) -> Result<Program, PatchError> {
        let len = self.measure().ok_or(PatchError::NotEnded)?;
        let mut map = MmapMut::map_anon(len)?;
        let base = map.as_ptr() as usize;
        self.finalize_into(&mut map[..], base, 0, &Default::default())?;
        let map = map.make_exec()?;

        Ok(Program {
            mmap: map,
            stack_slots: 0,
        })
    }
}
