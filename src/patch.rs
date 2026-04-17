use alloc::{vec, vec::Vec};

use patchouly_core::{
    Stencil, StencilFamily, StencilLibrary,
    relocation::{DelayedRelocation, DelayedTarget, JumpTarget, PatchInfo},
    stencils::{Location, SelectedStencil},
};
use smallvec::SmallVec;

use crate::PatchError;
#[cfg(feature = "std")]
use crate::{EntrypointSignature, Program, TypedProgram};

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

    /// Emit one plain, control-flow-free stencil.
    pub fn emit<const IN: usize, const OUT: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, 1>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        if self.ended {
            return Err(PatchError::AlreadyEnded);
        }
        self.emit_one(stencil, inputs, outputs, holes, &[JumpTarget::Next])
    }

    /// Emits a stencil using two value slots as inputs
    pub fn emit_vv(
        &mut self,
        stencil: &StencilFamily<2, 1, MAX_REGS, 0, 1>,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), PatchError> {
        self.emit(stencil, &[src1, src2], &[dst], &[])
    }

    /// Emits a stencil using a value slot as inputs
    pub fn emit_v(
        &mut self,
        stencil: &StencilFamily<1, 1, MAX_REGS, 0, 1>,
        src: Location,
        dst: Location,
    ) -> Result<(), PatchError> {
        self.emit(stencil, &[src], &[dst], &[])
    }

    /// Emits a stencil with a value slot and an immediate as inputs
    pub fn emit_vi(
        &mut self,
        stencil: &StencilFamily<1, 1, MAX_REGS, 1, 1>,
        src: Location,
        imm: usize,
        dst: Location,
    ) -> Result<(), PatchError> {
        self.emit(stencil, &[src], &[dst], &[imm])
    }

    /// Emits a return stencil.
    pub fn ret<const IN: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, 0, MAX_REGS, HOLES, 0>,
        inputs: &[Location; IN],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        self.ended = true;
        self.emit_one(stencil, inputs, &[], holes, &[])
    }

    /// Emits a single-slot return stencil.
    pub fn ret_v(
        &mut self,
        stencil: &StencilFamily<1, 0, MAX_REGS, 0, 0>,
        src: Location,
    ) -> Result<(), PatchError> {
        self.ret(stencil, &[src], &[])
    }

    /// Emits a final branching stencil.
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

    /// Emits a branching stencil, but does not end the block.
    pub fn branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Result<(), PatchError> {
        self.emit_one(stencil, inputs, outputs, holes, jumps)
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
    fn emit_one<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
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
                self.library.rt_symbols,
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
        let runtime_trampolines = self.runtime_trampoline_symbols();
        let code_len = self.code.len()
            + runtime_trampolines.len()
                * self
                    .long_jump_stencil()
                    .map_or(0, |stencil| stencil.code_len as usize);
        let constant_count = self.constants.len() + runtime_trampolines.len();
        if constant_count == 0 {
            code_len
        } else {
            code_len.next_multiple_of(size_of::<usize>()) + constant_count * size_of::<usize>()
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
        let block_base = base + block_base_offset;
        let runtime_trampolines = self.runtime_trampoline_symbols();
        let mut trampoline_addresses = vec![0usize; self.library.rt_symbols.len()];
        let mut trampoline_relocations = Vec::with_capacity(runtime_trampolines.len());
        let mut current = self.code.len();

        code[..self.code.len()].copy_from_slice(&self.code);
        if !runtime_trampolines.is_empty() {
            let long_jump = self
                .long_jump_stencil()
                .ok_or(PatchError::StencilNotFound)?;
            for (i, symbol) in runtime_trampolines.iter().copied().enumerate() {
                let offset = current;
                let stencil_code = long_jump.code(self.library.code);
                let next = current + stencil_code.len();
                code[current..next].copy_from_slice(stencil_code);
                trampoline_addresses[symbol as usize] = block_base + offset;

                for relocation in long_jump.relocations(self.library.long_jump) {
                    if relocation.is_invalid() {
                        break;
                    }
                    if let Some(PatchInfo::Hole(0)) = relocation.patch_info() {
                        trampoline_relocations.push(DelayedRelocation::constant(
                            offset,
                            *relocation,
                            (self.constants.len() + i) as u16,
                        ));
                    }
                }
                current = next;
            }
        }

        let constant_base = if self.constants.is_empty() && runtime_trampolines.is_empty() {
            current
        } else {
            let base = current.next_multiple_of(size_of::<usize>());
            let mut start = base;
            for constant in &self.constants {
                let bytes = &constant.to_ne_bytes();
                let next = start + bytes.len();
                code[start..next].copy_from_slice(bytes);
                start = next;
            }
            for symbol in runtime_trampolines.iter().copied() {
                let bytes = &(self.library.rt_symbols[symbol as usize] as usize).to_ne_bytes();
                let next = start + bytes.len();
                code[start..next].copy_from_slice(bytes);
                start = next;
            }
            base
        };

        for relocation in self.relocations.iter().chain(trampoline_relocations.iter()) {
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
                DelayedTarget::Runtime(symbol) => trampoline_addresses[symbol as usize],
                DelayedTarget::Next => unreachable!(),
            };
            relocation.apply(code, relocation.resolve(block_base, target));
        }

        Ok(())
    }

    #[cfg(feature = "std")]
    pub fn finalize(self) -> Result<Program, PatchError> {
        let len = self.measure().ok_or(PatchError::NotEnded)?;
        let mut map = memmap2::MmapMut::map_anon(len)?;
        let base = map.as_ptr() as usize;
        self.finalize_into(&mut map[..], base, 0, &Default::default())?;
        let map = map.make_exec()?;

        Ok(Program {
            mmap: map,
            stack_slots: 0,
        })
    }

    #[cfg(feature = "std")]
    pub fn finalize_typed<Sig: EntrypointSignature>(self) -> Result<TypedProgram<Sig>, PatchError> {
        self.finalize().map(Program::into_typed)
    }

    fn runtime_trampoline_symbols(&self) -> Vec<u16> {
        let mut symbols = vec![];
        for relocation in &self.relocations {
            let DelayedTarget::Runtime(symbol) = relocation.target() else {
                continue;
            };
            if !symbols.contains(&symbol) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    fn long_jump_stencil(&self) -> Option<&'static Stencil<0, 0, 1, 0>> {
        self.library
            .long_jump
            .select(&[], &[], &[usize::MAX])
            .map(|selected| selected.stencil)
    }
}
