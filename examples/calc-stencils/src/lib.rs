#![allow(incomplete_features)]
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]

use std::mem::MaybeUninit;

#[macro_use]
extern crate patchouly_macros;

struct Stack(Vec<MaybeUninit<usize>>);
impl Stack {
    fn get(&self, i: usize) -> usize {
        let i = self.0.len() - i - 1;
        unsafe { self.0[i].assume_init() }
    }
    fn set(&mut self, i: usize, v: usize) {
        let i = self.0.len() - i - 1;
        self.0[i] = MaybeUninit::new(v);
    }
    fn allocate(&mut self, len: usize) {
        self.0.reserve(len);
        unsafe {
            self.0.set_len(self.0.len() + len);
        }
    }
    fn pop_n(&mut self, n: usize) {
        unsafe {
            self.0.set_len(self.0.len() - n);
        }
    }
}

#[stencil]
fn stack_alloc(#[stack] stack: &mut Stack, #[hole] n: usize) {
    stack.allocate(n);
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
