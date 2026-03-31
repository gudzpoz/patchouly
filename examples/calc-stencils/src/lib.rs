#![allow(incomplete_features)]
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]

use std::mem::MaybeUninit;

use patchouly_core::StencilStack;

#[macro_use]
extern crate patchouly_macros;

pub struct Stack(pub Vec<MaybeUninit<usize>>);
impl StencilStack for Stack {
    #[inline]
    fn get(&self, i: usize) -> usize {
        let i = self.0.len() - i - 1;
        unsafe { self.0.get_unchecked(i).assume_init() }
    }
    #[inline]
    fn set(&mut self, i: usize, v: usize) {
        let i = self.0.len() - i - 1;
        unsafe {
            self.0.get_unchecked_mut(i).write(v);
        }
    }
}
impl Stack {
    // TODO: extern "rust-preserve-none" seems to clobber registers
    pub fn allocate(&mut self, len: usize) {
        self.0.reserve(len);
        unsafe {
            self.0.set_len(self.0.len() + len);
        }
    }
    #[inline]
    fn fast_allocate(&mut self, len: usize) -> bool {
        if self.0.capacity() >= self.0.len() + len {
            unsafe {
                self.0.set_len(self.0.len() + len);
            }
            true
        } else {
            false
        }
    }
    #[inline]
    fn pop_n(&mut self, n: usize) {
        unsafe {
            self.0.set_len(self.0.len() - n);
        }
    }
}
pub struct StackAllocFn(pub fn(&mut Stack, usize));
impl From<usize> for StackAllocFn {
    fn from(v: usize) -> Self {
        StackAllocFn(unsafe { std::mem::transmute::<usize, for<'a> fn(&'a mut Stack, usize)>(v) })
    }
}
impl From<StackAllocFn> for usize {
    fn from(val: StackAllocFn) -> Self {
        val.0 as usize
    }
}

setup_stencils!("Calc");

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
