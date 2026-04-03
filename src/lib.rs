#![doc = include_str!("../README.md")]
#![no_std]
#![feature(rust_preserve_none_cc)]

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
    /// Converts this executable mapping into a typed program entrypoint.
    pub fn into_typed<Sig: EntrypointSignature>(self) -> TypedProgram<Sig> {
        let entry = unsafe { Sig::from_ptr(self.mmap.as_ptr()) };
        TypedProgram {
            program: self,
            entry,
        }
    }

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

/// Typed executable entrypoint
///
/// ## Signature requirements
///
/// `Sig` are currently generated from:
/// - `extern "rust-preserve-none" fn(&mut ()) -> usize` (aliased to [`RawFn0`])
/// - up to:
/// - `extern "rust-preserve-none" fn(&mut (), usize, usize, usize, usize, usize, usize) -> usize`
///   (aliased to [`RawFn6`])
///
/// Note that it is still possible to specify any signature you want
/// (like multiple return values) with unsafe casting.
///
/// ```compile_fail
/// use patchouly::EntrypointSignature;
///
/// fn assert_supported<Sig: EntrypointSignature>() {}
///
/// // `extern "C"` signatures are rejected at compile time.
/// assert_supported::<extern "C" fn(&mut (), usize) -> usize>();
/// ```
///
/// ```no_run
/// #![feature(rust_preserve_none_cc)]
/// # use patchouly::{RawFn1, patch::PatchBlock};
/// # fn build_block() -> PatchBlock<10> { todo!() }
/// # fn demo() -> Result<(), patchouly::PatchError> {
/// let block = build_block();
///
/// let program = block
///     .finalize_typed::<RawFn1<()>>()?;
/// let result = unsafe {program.entry()}(&mut (), 1);
/// let _ = result;
/// # Ok(())
/// # }
/// ```
pub struct TypedProgram<Sig: EntrypointSignature> {
    program: Program,
    entry: Sig,
}

impl<Sig: EntrypointSignature> TypedProgram<Sig> {
    /// Returns the entrypoint function
    ///
    /// # Safety
    ///
    /// Well, you compiled it, so you know what you're doing.
    /// It's unsafe.
    pub unsafe fn entry(&self) -> Sig {
        self.entry
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn into_program(self) -> Program {
        self.program
    }
}

impl<Sig: EntrypointSignature> Deref for TypedProgram<Sig> {
    type Target = Program;

    fn deref(&self) -> &Self::Target {
        &self.program
    }
}

pub trait EntrypointSignature: sealed::Sealed + Copy + 'static {
    #[doc(hidden)]
    unsafe fn from_ptr(ptr: *const u8) -> Self;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! impl_entrypoint_signature {
    ($name:ident $(,$arg:ty)*) => {
        pub type $name<T> = extern "rust-preserve-none" fn(&mut T, $($arg),*) -> usize;

        impl<T: 'static> sealed::Sealed for $name<T> {}

        impl<T: 'static> EntrypointSignature for $name<T> {
            unsafe fn from_ptr(ptr: *const u8) -> Self {
                unsafe { transmute::<*const u8, Self>(ptr) }
            }
        }
    };
}

impl_entrypoint_signature!(RawFn0);
impl_entrypoint_signature!(RawFn1, usize);
impl_entrypoint_signature!(RawFn2, usize, usize);
impl_entrypoint_signature!(RawFn3, usize, usize, usize);
impl_entrypoint_signature!(RawFn4, usize, usize, usize, usize);
impl_entrypoint_signature!(RawFn5, usize, usize, usize, usize, usize);
impl_entrypoint_signature!(RawFn6, usize, usize, usize, usize, usize, usize);

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
