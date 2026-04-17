#![allow(incomplete_features)]
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]
#![allow(clippy::too_many_arguments)]

use example_commons::{BoxedVec, Stack, StackAllocFn};
use patchouly_core::StencilStack;

#[macro_use]
extern crate patchouly_macros;

setup_stencils!(name = "Calc");

#[stencil]
fn stack_alloc(#[stack] stack: &mut Stack, #[hole] allocate: StackAllocFn, #[hole] n: usize) {
    if !stack.fast_allocate(n) {
        allocate.0(stack, n);
    }
}
#[stencil]
fn stack_pop(#[stack] stack: &mut Stack, #[hole] n: usize) {
    stack.pop_n(n);
}
#[stencil]
fn add1(a: usize) -> usize {
    a + 1
}
#[stencil]
fn add_const(a: usize, #[hole] c: usize) -> usize {
    a + c
}
#[stencil]
fn add(a: usize, b: usize) -> usize {
    a + b
}
#[stencil(returns)]
fn ret(a: usize) -> usize {
    a
}

#[stencil]
fn if_eq(a: usize, #[hole] c: usize, #[target] then: usize, #[target] or_else: usize) {
    if a == c { then } else { or_else }
}

#[stencil]
fn vec_sum(v: BoxedVec) -> usize {
    v.0.iter().sum()
}
