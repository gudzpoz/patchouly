pub mod relocation;
pub mod stencils;

/// A trait for a stencil stack
///
/// The compiler will generate code that uses stacks
/// when a variable is determined to be "spilled".
///
/// ## Requirements
///
/// Note that the functions must be annotated with `#[inline]`
/// and must not involve further linkage. `patchouly-build` will
/// report these violations on build-time.
pub trait StencilStack {
    fn get(&self, i: usize) -> usize;
    fn set(&mut self, i: usize, v: usize);
}

/// A library of stencils
pub struct StencilLibrary {
    /// All the stencil binary code, referred to by stencils
    pub code: &'static [u8],
    /// The code for an empty stencil, used to prune consecutive jumps
    pub empty: &'static [u8],
    /// Number of available registers
    pub registers: u16,
}

pub use stencils::Stencil;
pub use stencils::StencilFamily;

pub use stencils::StencilFamilyBuild;
