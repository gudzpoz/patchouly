mod perm;
mod setup;
mod stencil;

use proc_macro::TokenStream;
use syn::{LitStr, parse_macro_input};

use crate::stencil::StencilFamily;

/// Generates a family of stencils, to be used with `patchouly_build/_core`.
///
/// ## Usage by examples
///
/// ```rust
/// // These are required to use the required experimental features:
/// #![allow(incomplete_features)]
/// #![feature(explicit_tail_calls)]
/// #![feature(rust_preserve_none_cc)]
///
/// # mod test { // needs to be in a module
/// use patchouly_core::StencilStack;
/// use patchouly_macros::{setup_stencils, stencil};
///
/// /// A type named exactly `Stack` must be in scope:
/// pub struct Stack();
/// /// And it must implement `patchouly_core::StencilStack`:
/// impl StencilStack for Stack {
///     #[inline(always)] fn get(&self, i: usize) -> usize { todo!() }
///     #[inline(always)] fn set(&mut self, i: usize, v: usize) { todo!() }
/// }
/// # impl Stack {
/// #     #[inline(always)] fn inlinable_fast_allocate(&mut self, n: usize) -> bool { todo!() }
/// # }
///
/// setup_stencils!("Calc");
///
/// /// Annotate it on any function:
/// #[stencil]
/// fn add1(a: usize) -> usize {
///     a + 1
/// }
///
/// /// Use `#[hole]` to specify compile-time constants:
/// #[stencil]
/// fn add_const(a: usize, #[hole] c: usize) -> usize {
///     a + c
/// }
///
/// # struct SlowAllocFn(pub fn(&mut Stack, usize));
/// # impl From<usize> for SlowAllocFn {
/// #     fn from(n: usize) -> Self { todo!() }
/// # }
///
/// /// Use `#[stack]` to access the stack:
/// #[stencil]
/// fn stack_reserve(
///     #[stack] stack: &mut Stack,
///     #[hole] slow_alloc: SlowAllocFn,
///     #[hole] n: usize,
/// ) {
///     // All function calls must be inlined
///     if !stack.inlinable_fast_allocate(n) {
///         // Non-inlined functions must be externally supplied
///         slow_alloc.0(stack, n);
///     }
/// }
///
/// /// Use `#[target]` to define a control flow target:
/// #[stencil]
/// fn if_else(cond: usize, #[target] then: _, #[target] or_else: _) -> _ {
///     /// You can only return jump targets here
///     if cond == 0 { then } else { or_else }
/// }
///
/// /// Use `#[stencil(returns)]` to define a returning stencil:
/// #[stencil(returns)]
/// fn return_here(return_value: usize) -> usize {
///     return_value
/// }
/// # }
/// ```
///
/// ## Argument/return types
///
/// The generated code uses [Into] to convert between users' types and [usize],
/// so as long as your types implement [Into]/[From] for [usize], you can use them.
#[proc_macro_attribute]
pub fn stencil(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(item as StencilFamily);
    if let Err(err) = function.set_options(attr.into()) {
        return err.into_compile_error().into();
    }
    function.expand().into()
}

/// Generates a few things needed by `patchouly_build` to extract stencils.
///
/// Each stencil crate should call this exactly once.
///
/// ## Usage
///
/// See [`stencil`].
///
/// ```rust
/// #![allow(incomplete_features)]
/// #![feature(explicit_tail_calls)]
/// #![feature(rust_preserve_none_cc)]
///
/// # mod test {
/// use patchouly_macros::setup_stencils;
///
/// # use patchouly_core::StencilStack;
/// # pub struct Stack;
/// # impl StencilStack for Stack {
/// #     fn get(&self, i: usize) -> usize { todo!() }
/// #     fn set(&mut self, i: usize, v: usize) { todo!() }
/// # }
///
/// setup_stencils!("Calc"); // prefix of the generated bindings
/// # }
/// ```
#[proc_macro]
pub fn setup_stencils(input: TokenStream) -> TokenStream {
    let name = parse_macro_input!(input as LitStr);
    setup::setup(name).into()
}
