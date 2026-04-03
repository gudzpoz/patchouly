# patchouly-build

This crate provides a helper for extracting stencils from
a `patchouly-macros` stencils crate.

## Usage

We expect to be in a Cargo workspace, with the two crates in the same directory:

```bash
$ tree crates/
crates
├── bf
│   ├── build.rs    <-- where we use patchouly-build
│   ├── Cargo.toml
│   └── src
│       └── main.rs
└── bf-stencils
    ├── Cargo.toml
    └── src
        └── lib.rs  <-- the #[stencil] definitions
```

Then, use [StencilSetup] to extract the stencils:

```rust,ignore
// build.rs
StencilSetup::new("bf-stencils")
    .extract_and_emit()
    .expect("failed to extract stencils");
```

And use `patchouly::include_stencils!()` to include them in your crate:

```rust,ignore
// lib.rs
patchouly::include_stencils!();
```
