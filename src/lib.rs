use std::{
    fmt::{Debug, Write},
    io,
    mem::size_of,
    ops::Deref,
};

use memmap2::{Mmap, MmapMut};
use patchouly_core::{
    Stencil, StencilFamily, StencilLibrary,
    relocation::{DelayedRelocation, DelayedTarget, JumpTarget, PatchInfo},
    stencils::Location,
};
use smallvec::SmallVec;
use thiserror::Error;

#[derive(Default)]
pub struct ProgramBlocks {
    // TODO
}
impl ProgramBlocks {
    fn resolve_target(&self, _block: u16) -> usize {
        todo!()
    }
}

#[derive(Error, Debug)]
pub enum PatchError {
    #[error("invalid register")]
    InvalidRegister,
    #[error("patch block must be ended with an returning or switching stencil")]
    NotEnded,
    #[error("last stencil was not returning or switching")]
    InvalidEnd,
    #[error("unable to mmap")]
    MmapError(#[from] io::Error),
}

pub struct PatchBlock<const MAX_REGS: usize> {
    library: &'static StencilLibrary<MAX_REGS>,
    code: Vec<u8>,
    relocations: Vec<DelayedRelocation>,
    constants: Vec<usize>,
    next_relocations: Vec<DelayedRelocation>,
    ended: bool,
}

struct PatchArgs<'a, const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
    pub &'a [Location; IN],
    pub &'a [Location; OUT],
    pub &'a [usize; HOLES],
    pub &'a [JumpTarget; JUMPS],
    pub bool,
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
            return Err(PatchError::NotEnded);
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

    pub fn branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Result<(), PatchError> {
        self.ended = true;
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
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
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
        let (wide, s) = stencil.select(inputs, outputs, holes);

        let next_target = self.code.len();
        if let Some(last) = self.next_relocations.pop() {
            last.apply(&mut self.code, last.resolve(0, next_target));
        }
        if self.code.ends_with(self.library.empty) {
            self.code
                .truncate(self.code.len() - self.library.empty.len());
        }
        let next_target = self.code.len();
        for relocation in self.next_relocations.drain(..) {
            relocation.apply(&mut self.code, relocation.resolve(0, next_target));
        }

        let from = self.code.len();
        self.copy_and_patch::<IN, OUT, HOLES, JUMPS>(
            stencil,
            s,
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
        if self.ended {
            if self.constants.is_empty() {
                Some(self.code.len())
            } else {
                Some(
                    self.code.len().next_multiple_of(size_of::<usize>())
                        + self.constants.len() * size_of::<usize>(),
                )
            }
        } else {
            None
        }
    }

    pub fn finalize(self, program: &ProgramBlocks) -> Result<Program, PatchError> {
        if !self.ended {
            return Err(PatchError::NotEnded);
        }
        if !self.next_relocations.is_empty() {
            // last stencil trying to jump over block end
            return Err(PatchError::InvalidEnd);
        }

        let mut code = self.code;
        let constant_base = if self.constants.is_empty() {
            code.len()
        } else {
            code.resize(code.len().next_multiple_of(size_of::<usize>()), 0);
            let base = code.len();
            for constant in self.constants {
                code.extend_from_slice(&constant.to_ne_bytes());
            }
            base
        };

        let mut map = MmapMut::map_anon(code.len())?;
        map.copy_from_slice(&code);
        let base = map.as_ptr() as usize;
        for relocation in self.relocations {
            let target = match relocation.target() {
                DelayedTarget::Block(block) => program.resolve_target(block),
                DelayedTarget::Constant(index) => {
                    base + constant_base + index as usize * size_of::<usize>()
                }
                DelayedTarget::Next => unreachable!(),
            };
            relocation.apply(&mut map[..], relocation.resolve(base, target));
        }
        let map = map.make_exec()?;

        Ok(Program { mmap: map })
    }
}

pub struct Program {
    mmap: Mmap,
}

impl Program {
    pub fn as_slice(&self) -> &[u8] {
        self.mmap.deref()
    }

    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.mmap.as_ptr()
    }
}

impl Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct ByteLiteral<'a>(&'a [u8]);
        let s = ByteLiteral(self.mmap.deref());
        impl Debug for ByteLiteral<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_char('b')?;
                f.write_char('"')?;
                for b in self.0 {
                    f.write_fmt(format_args!("\\{:02x}", b))?;
                }
                f.write_char('"')
            }
        }

        f.debug_struct("Program").field("mmap", &s).finish()
    }
}
