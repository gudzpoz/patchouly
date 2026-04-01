pub mod patch;

use std::{
    fmt::{Debug, Write},
    io,
    ops::Deref,
};

use memmap2::Mmap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PatchError {
    #[error("invalid register")]
    InvalidRegister,
    #[error("stencil not found")]
    StencilNotFound,
    #[error("patch block must be ended with an returning or switching stencil")]
    NotEnded,
    #[error("last stencil was not returning or switching")]
    InvalidEnd,
    #[error("block target not found")]
    UnresolvedBlockTarget,
    #[error("unable to mmap")]
    MmapError(#[from] io::Error),
}

pub struct Program {
    mmap: Mmap,
    _stack_slots: usize,
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

    pub fn base_address(&self) -> usize {
        self.mmap.as_ptr() as usize
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
