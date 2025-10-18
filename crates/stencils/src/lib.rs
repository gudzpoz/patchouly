#![feature(explicit_tail_calls)]
#![expect(incomplete_features)]

#[macro_use]
extern crate stencil_macros;

#[stencil]
pub fn load_int_reg1(#[hole] reg1: usize) -> usize {
    reg1
}
#[stencil]
pub fn load_int_reg2(noop: usize, #[hole] reg2: usize) -> (usize, usize) {
    (noop, reg2)
}
#[stencil]
pub fn add_int1_int2(a: usize, b: usize) -> (usize, usize) {
    (a + b, b)
}
#[stencil(return)]
pub fn return_int1(a: usize) -> usize {
    a
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
mod tests {
    #[stencil]
    pub fn test_abi(
        a: usize, b: usize, c: usize, d: usize, e: usize, f: usize, g: usize, h: usize,
        i: usize, j: usize, k: usize, l: usize, m: usize, n: usize, o: usize, p: usize,
    )
    -> (
        usize, usize, usize, usize, usize, usize, usize, usize,
        usize, usize, usize, usize, usize, usize, usize, usize,
    ) {
        (p, a, b, c, d, e, f, g, h, i, j, k, l, m, n, o)
    }
}
