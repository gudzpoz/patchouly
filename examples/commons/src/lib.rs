use std::mem::MaybeUninit;

use patchouly_core::StencilStack;

#[derive(Default)]
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
    pub fn allocate(&mut self, len: usize) {
        self.0.reserve(len);
        unsafe {
            self.0.set_len(self.0.len() + len);
        }
    }
    #[inline]
    pub fn fast_allocate(&mut self, len: usize) -> bool {
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
    pub fn pop_n(&mut self, n: usize) {
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
