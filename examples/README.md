# Copy-and-patch JIT Examples

Here are some examples of building JIT engines with [Patchouly]:

- [`calc`](./calc): some basic arithmetic, using a lower-level API;
- [`brainfuck`](./bf): a brainfuck runtime, using a bit higher-level API.

[Patchouly]: https://github.com/gudzpoz/patchouly

## Structures of Copy-and-patch Crates

To use Patchouly, you need at lease two crates: one holding the stencils,
and the other one using the stencils to build a JIT engine.
The examples here all follow the same pattern, except that they add another
`commons` crate to hold shared code, so that the JIT engine crate don't need
to depend on the stencil crate: it depends on the stencil binary, but not
directly on the Rust code.

| Example | Stencils crate | JIT engine crate |
|---|---|---|
| `calc` | [`calc-stencils`](./calc-stencils) | [`calc`](./calc) |
| `brainfuck` | [`bf-stencils`](./bf-stencils) | [`bf`](./bf) |

### The stencils crate

The stencils crate typically depends on `patchouly-core` and `patchouly-macros`,
and contains the stencils used by the JIT engine, marked with the `#[stencil]`
attribute.

You also need to use `setup_stencils` macro from `patchouly-macros` to generate
some things needed by `patchouly-build` to extract the stencils.

### The JIT engine crate

The JIT engine crate typically depends on `patchouly` and `patchouly-core`,
with `patchouly-build` as an additional dev-dependency, used in `build.rs`
to extract the compiled stencils.

The `patchouly` crate contains some helper functions to emit and link the
stencils into executable code. But they are rather basic and simple: you can
build your own if you need more control.

For example, the `bf` crate has the following in its `build.rs`:

```rust
use patchouly_build::extract;

fn main() {
    extract("bf-stencils").unwrap();
}
```

And it uses `include!` to include the generated typed stencils:

```rust
#![feature(rust_preserve_none_cc)]
include!(concat!(env!("OUT_DIR"), "/bf_stencils.rs"));
```

Then, you can access the stencils from the `include!`-ed `stencils` module:

```rust
let mut builder = PatchFunctionBuilder::new(&stencils::BF_STENCIL_LIBRARY);
```
