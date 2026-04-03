#![doc = include_str!("../README.md")]

#![no_std]

pub mod managed;
pub mod patch;
mod regs;

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

/// Includes generated stencil bindings emitted by `patchouly-build`.
///
/// Pair this with `patchouly_build::StencilSetup::extract_and_emit()` in
/// `build.rs`, which exports the `PATCHOULY_STENCILS_RS` env var.
#[macro_export]
macro_rules! include_stencils {
    () => {
        include!(env!("PATCHOULY_STENCILS_RS"));
    };
}

use core::fmt::{Debug, Write};
use core::mem::transmute;
use core::ops::Deref;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PatchError {
    #[error("invalid register")]
    InvalidRegister,
    #[error("stencil not found")]
    StencilNotFound,
    #[error("patch block must be ended with an returning or switching stencil")]
    NotEnded,
    #[error("patch block already ended")]
    AlreadyEnded,
    #[error("last stencil was not returning or switching")]
    InvalidEnd,
    #[error("block target not found")]
    UnresolvedBlockTarget,
    #[error("a block must be branched into before emitting code")]
    BlockOutOfScope,
    #[error("one can only add params for the entry block")]
    InvalidParams,
    #[error("variable not found in current scope")]
    VariableOutOfScope,
    #[error("unable to allocate slot for variable")]
    OutOfVariables,
    #[cfg(feature = "std")]
    #[error("unable to mmap")]
    MmapError(#[from] std::io::Error),
}

pub struct Program {
    #[cfg(feature = "std")]
    mmap: memmap2::Mmap,
    #[cfg(not(feature = "std"))]
    mmap: alloc::vec::Vec<u8>,
    pub stack_slots: u16,
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
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        struct ByteLiteral<'a>(&'a [u8]);
        let s = ByteLiteral(self.mmap.deref());
        impl Debug for ByteLiteral<'_> {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
