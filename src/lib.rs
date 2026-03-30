use std::{error::Error, fmt::{Debug, Write}, ops::Deref};

use memmap2::{Mmap, MmapMut};
use patchouly_core::{StencilFamily, StencilLibrary, stencils::Variable};

pub struct PatchBlock {
    library: &'static StencilLibrary,
    code: Vec<u8>,
}

impl PatchBlock {
    pub fn new(library: &'static StencilLibrary) -> Self {
        Self {
            library,
            code: vec![],
        }
    }

    pub fn emit<
        const IN: usize,
        const OUT: usize,
        const MAX_REGS: usize,
        const HOLES: usize,
        const INPUTS: usize,
    >(
        &mut self, stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, INPUTS>,
        inputs: &[Variable; IN],
        outputs: &[Variable; OUT],
        holes: &[usize; HOLES],
        jumps: &[usize; INPUTS],
    ) -> Option<()> {
        if inputs.iter().any(|v| v.into_bits() >= self.library.registers)
            || outputs.iter().any(|v| v.into_bits() >= self.library.registers)
        {
            return None;
        }
        let s = stencil.select(inputs, outputs);

        if self.code.ends_with(self.library.empty) {
            self.code.truncate(self.code.len() - self.library.empty.len());
        }
        let from = self.code.len();
        s.copy(self.library.code, &mut self.code);
        s.patch(stencil, inputs, outputs, holes, jumps, &mut self.code[from..]);

        Some(())
    }

    pub fn finalize(self) -> Result<Program, Box<dyn Error>> {
        let code = self.code;

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

        f.debug_struct("Program")
            .field("mmap", &s)
            .finish()
    }
}
