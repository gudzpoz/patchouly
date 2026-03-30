mod patch;

use std::{
    error::Error,
    fmt::{Debug, Write},
    ops::Deref,
};

use memmap2::{Mmap, MmapMut};
use patchouly_core::{StencilFamily, StencilLibrary, stencils::Variable};

pub use crate::patch::JumpTarget;
use crate::patch::{CopyNPatch, DelayedRelocation, PatchArgs};

#[derive(Default)]
pub struct ProgramBlocks {
    // TODO
}
impl ProgramBlocks {
    fn resolve_target(&self, _block: u16) -> usize {
        todo!()
    }
}

pub struct PatchBlock<const MAX_REGS: usize> {
    library: &'static StencilLibrary<MAX_REGS>,
    code: Vec<u8>,
    relocations: Vec<DelayedRelocation>,
    ended: bool,
}

impl<const MAX_REGS: usize> PatchBlock<MAX_REGS> {
    pub fn new(library: &'static StencilLibrary<MAX_REGS>) -> Self {
        Self {
            library,
            code: vec![],
            relocations: vec![],
            ended: false,
        }
    }

    pub fn add<const IN: usize, const OUT: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, 1>,
        inputs: &[Variable; IN],
        outputs: &[Variable; OUT],
        holes: &[usize; HOLES],
    ) -> Option<()> {
        if self.ended {
            // TODO: error reporting
            return None;
        }
        self.emit(stencil, inputs, outputs, holes, &[JumpTarget::Next])
    }

    pub fn ret<const IN: usize, const HOLES: usize>(
        &mut self,
        stencil: &StencilFamily<IN, 0, MAX_REGS, HOLES, 0>,
        inputs: &[Variable; IN],
        holes: &[usize; HOLES],
    ) -> Option<()> {
        self.ended = true;
        self.emit(stencil, inputs, &[], holes, &[])
    }

    pub fn branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Variable; IN],
        outputs: &[Variable; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Option<()> {
        self.ended = true;
        self.emit(stencil, inputs, outputs, holes, jumps)
    }

    fn emit<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Variable; IN],
        outputs: &[Variable; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpTarget; JUMPS],
    ) -> Option<()> {
        if inputs.iter().any(|v| v.into_bits() as usize >= MAX_REGS)
            || outputs.iter().any(|v| v.into_bits() as usize >= MAX_REGS)
        {
            return None;
        }
        let s = stencil.select(inputs, outputs);

        if self.code.ends_with(self.library.empty) {
            self.code
                .truncate(self.code.len() - self.library.empty.len());
            self.relocations.pop();
        }
        let from = self.code.len();
        s.copy(self.library.code, &mut self.code);
        s.patch(
            stencil,
            PatchArgs(inputs, outputs, holes, jumps),
            &mut self.code,
            from,
            &mut self.relocations,
        );

        Some(())
    }

    pub fn finalize(self, program: &ProgramBlocks) -> Result<Program, Box<dyn Error>> {
        if !self.ended {
            // TODO: error reporting
            return Err("not ended".into());
        }

        let mut code = self.code;

        for relocation in self.relocations {
            relocation.apply(
                &mut code,
                program
                    .resolve_target(relocation.target())
                    .wrapping_sub(relocation.offset()) as isize,
            );
        }

        let mut map = MmapMut::map_anon(code.len())?;
        map.copy_from_slice(&code);
        let map = map.make_exec()?;

        Ok(Program { mmap: map })
    }
}

pub struct Program {
    mmap: Mmap,
}

impl Program {
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
