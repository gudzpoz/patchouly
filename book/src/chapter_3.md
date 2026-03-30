# Register Allocation in Copy-and-patch

```rust
#![feature(explicit_tail_calls)]
#![feature(rust_preserve_none_cc)]
#![allow(incomplete_features)]
# mod tests {
use patchouly_macros::stencil;
# struct Stack();
# impl Stack {
#   fn get(&self, i: usize) -> usize { todo!() }
#   fn set(&self, i: usize, v: usize) -> usize { todo!() }
# }

#[stencil]
fn add1(a: usize) -> usize {
  a + 1
}
# } // mod tests
```

> Currently we need the experimental ["rust-preserve-none" ABI]
> to be able to use `extern "rust-preserve-none"` for our patchouly stencils.

["rust-preserve-none" ABI]: https://github.com/rust-lang/rust/issues/151401
