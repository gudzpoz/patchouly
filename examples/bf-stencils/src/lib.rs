#![allow(incomplete_features)]
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]

use patchouly_core::StencilStack;
use patchouly_macros::{setup_stencils, stencil};

setup_stencils!(name = "BF", n = 4);

struct Stack();
impl StencilStack for Stack {
    #[inline(always)]
    fn get(&self, _i: usize) -> usize {
        0
    }
    #[inline(always)]
    fn set(&mut self, _i: usize, _v: usize) {}
}

pub struct Ptr(*mut u8);
impl From<usize> for Ptr {
    fn from(v: usize) -> Self {
        Ptr(v as *mut u8)
    }
}
impl Ptr {
    fn to_slice(&self, len: usize) -> &'static mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0, len) }
    }
    unsafe fn get_mut(&mut self, i: usize) -> &mut u8 {
        unsafe { self.0.add(i).as_mut().unwrap_unchecked() }
    }
}

#[stencil(n = 4)]
fn add1(mut dp: Ptr, _len: usize, i: usize) {
    *unsafe { dp.get_mut(i) } += 1;
}
#[stencil(n = 4)]
fn addn(mut dp: Ptr, _len: usize, i: usize, #[hole] n: usize) {
    *unsafe { dp.get_mut(i) } += n as u8;
}

#[stencil(n = 4)]
fn sub1(mut dp: Ptr, _len: usize, i: usize) {
    *unsafe { dp.get_mut(i) } -= 1;
}
#[stencil(n = 4)]
fn subn(mut dp: Ptr, _len: usize, i: usize, #[hole] n: usize) {
    *unsafe { dp.get_mut(i) } -= n as u8;
}

#[stencil(n = 4)]
fn check(_dp: Ptr, len: usize, i: usize, #[target] next: _, #[target] panic: _) {
    if i < len { next } else { panic }
}

#[stencil(n = 4)]
fn left1(i: usize) -> usize {
    i - 1
}
#[stencil(n = 4)]
fn leftn(i: usize, #[hole] n: usize) -> usize {
    i - n
}

#[stencil(n = 4)]
fn right1(i: usize) -> usize {
    i + 1
}
#[stencil(n = 4)]
fn rightn(i: usize, #[hole] n: usize) -> usize {
    i + n
}

#[derive(Clone, Copy)]
pub struct InputFn(pub fn() -> u8);
#[derive(Clone, Copy)]
pub struct OutputFn(pub fn(u8));
impl From<usize> for InputFn {
    fn from(v: usize) -> InputFn {
        InputFn(unsafe { std::mem::transmute::<usize, fn() -> u8>(v) })
    }
}
impl From<InputFn> for usize {
    fn from(val: InputFn) -> Self {
        val.0 as usize
    }
}
impl From<usize> for OutputFn {
    fn from(v: usize) -> OutputFn {
        OutputFn(unsafe { std::mem::transmute::<usize, fn(u8)>(v) })
    }
}
impl From<OutputFn> for usize {
    fn from(val: OutputFn) -> Self {
        val.0 as usize
    }
}

#[stencil(n = 4)]
fn print(mut dp: Ptr, _len: usize, i: usize, #[hole] print: OutputFn) {
    print.0(*unsafe { dp.get_mut(i) });
}
#[stencil(n = 4)]
fn read(mut dp: Ptr, _len: usize, i: usize, #[hole] read: InputFn) {
    *unsafe { dp.get_mut(i) } = read.0();
}

#[stencil(n = 4)]
fn if_zero(
    dp: Ptr,
    len: usize,
    i: usize,
    #[target] then: _,
    #[target] or_else: _,
    #[target] panic: _,
) {
    if let Some(v) = dp.to_slice(len).get(i) {
        if *v == 0 { then } else { or_else }
    } else {
        panic
    }
}

#[stencil(n = 4)]
fn jmp() {}

#[stencil(returns, n = 4)]
fn ret(#[hole] n: usize) -> usize {
    n
}

/// Special casing for `[-]`
#[stencil(n = 4)]
fn set_0(mut dp: Ptr, _len: usize, i: usize) {
    *unsafe { dp.get_mut(i) } = 0;
}
/// Special casing for `[->>>+<<<]`
#[stencil(n = 4)]
fn add_to(mut dp: Ptr, _len: usize, i: usize, #[hole] n: usize) {
    let v = std::mem::replace(unsafe { dp.get_mut(i) }, 0);
    *unsafe { dp.get_mut(i.wrapping_add_signed(n as isize)) } += v;
}
