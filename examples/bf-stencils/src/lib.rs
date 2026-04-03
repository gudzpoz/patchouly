#![allow(incomplete_features)]
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]

use example_commons::{InputFn, OutputFn, Stack};
use patchouly_core::StencilStack;
use patchouly_macros::{setup_stencils, stencil};

setup_stencils!(name = "BF", n = 4);

#[repr(transparent)]
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
fn left1_sat(i: usize) -> usize {
    i.saturating_sub(1)
}
#[stencil(n = 4)]
fn leftn_sat(i: usize, #[hole] n: usize) -> usize {
    i.saturating_sub(n)
}

#[stencil(n = 4)]
fn right1(i: usize) -> usize {
    i + 1
}
#[stencil(n = 4)]
fn rightn(i: usize, #[hole] n: usize) -> usize {
    i + n
}
#[stencil(n = 4)]
fn right1_sat(i: usize, #[hole] lim: usize) -> usize {
    i.saturating_add(1).min(lim)
}
#[stencil(n = 4)]
fn rightn_sat(i: usize, #[hole] n: usize, #[hole] lim: usize) -> usize {
    i.saturating_add(n).min(lim)
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
fn if_zero_unchecked(
    mut dp: Ptr,
    _len: usize,
    i: usize,
    #[target] then: _,
    #[target] or_else: _,
) {
    if *unsafe { dp.get_mut(i) } == 0 { then } else { or_else }
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
