#![doc = include_str!("../README.md")]

pub mod extract;
pub mod generate;
mod structs;

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Environment variable consumed by `patchouly::include_stencils!()`.
pub const STENCILS_RS_ENV: &str = "PATCHOULY_STENCILS_RS";

/// Output metadata for generated stencil bindings.
#[derive(Debug, Clone)]
pub struct ExtractOutput {
    /// Name declared via `setup_stencils!(name = "...")`.
    pub stencil_name: String,
    /// Absolute path to `<name>_stencils.rs` generated under `OUT_DIR`.
    pub stencils_rs: PathBuf,
}

/// Unified stencil setup helper for build scripts.
///
/// It validates the stencil crate identifier, extracts stencils, and
/// exports an env var to be consumed by `patchouly::include_stencils!()`.
///
/// ## End-to-end usage for a new example crate
///
/// ```rust,ignore
/// // build.rs
/// use patchouly_build::StencilSetup;
///
/// fn main() {
///     StencilSetup::new("calc-stencils")
///         .extract_and_emit()
///         .expect("failed to extract stencils");
/// }
/// ```
///
/// ```rust,ignore
/// // src/main.rs (or src/lib.rs)
/// #![feature(rust_preserve_none_cc)]
/// patchouly::include_stencils!();
///
/// fn main() {
///     let _ = &stencils::CALC_STENCIL_LIBRARY;
/// }
/// ```
#[derive(Debug, Clone)]
pub struct StencilSetup<'a> {
    rel_stencils_dir: &'a str,
    include_env: &'a str,
}

impl<'a> StencilSetup<'a> {
    pub fn new(rel_stencils_dir: &'a str) -> Self {
        Self {
            rel_stencils_dir,
            include_env: STENCILS_RS_ENV,
        }
    }

    pub fn with_include_env(mut self, include_env: &'a str) -> Result<Self, Box<dyn Error>> {
        if include_env.trim().is_empty() {
            return Err("include env var name must not be empty".into());
        }
        self.include_env = include_env;
        Ok(self)
    }

    pub fn extract_and_emit(self) -> Result<ExtractOutput, Box<dyn Error>> {
        let output = extract_with_output(self.rel_stencils_dir)?;
        println!(
            "cargo:rustc-env={}={}",
            self.include_env,
            output.stencils_rs.display()
        );
        Ok(output)
    }
}

/// Compiles and extracts stencils from a stencils crate
///
/// ## Usage
///
/// Note that the current implementation hardcodes a lot of things:
/// - The source directory of the stencils crate should be `src/`.
/// - The internal `.rlib` format used by Rust is assumed to be
///   located under some certain directories, named `libXXX.rlib`
///   and is an object file.
/// - The output directory of `cargo rustc --release` is assumed
///   to be under `$CARGO_TARGET_DIR/release/`.
///
/// If things above are met, this function will probably work by
/// compiling and extracting stencils into a `$OUT_DIR/{}_stencils.rs`
/// file, where `{}` is the lowercase of the name specified in your
/// `setup_stencils!(name = "...");` macro call in the stencils crate.
/// For example, `setup_stencils!(name = "Calc");` will generate a file
/// named `calc_stencils.rs`, which you may include with:
///
/// ```ignore
/// include!(concat!(env!("OUT_DIR"), "/calc_stencils.rs"));
/// ```
pub fn extract(rel_stencils_dir: &str) -> Result<(), Box<dyn Error>> {
    extract_with_output(rel_stencils_dir).map(|_| ())
}

/// Same as [`extract`], but also returns generated file metadata.
pub fn extract_with_output(rel_stencils_dir: &str) -> Result<ExtractOutput, Box<dyn Error>> {
    let out_dir = std::env::var("OUT_DIR")?;
    let out_path = Path::new(&out_dir).canonicalize()?;
    // We need a different target dir to prevent deadlock
    let target_dir = find_target_dir(&out_path)?.join("patchouly");
    let current_dir = std::env::current_dir()?;
    let stencils_dir = Path::new(rel_stencils_dir);

    let stencils_dir = if stencils_dir.is_absolute() {
        stencils_dir.to_path_buf()
    } else {
        current_dir
            .as_path()
            .parent()
            .ok_or("expected to be in a workspace")?
            .join(rel_stencils_dir)
    };
    if !stencils_dir.exists() {
        return Err(format!(
            "stencil crate `{rel_stencils_dir}` not found from `{}`; expected directory `{}`",
            current_dir.display(),
            stencils_dir.display()
        )
        .into());
    }
    let stencils_dir = stencils_dir.canonicalize()?;

    // compile
    let status = Command::new("cargo")
        .current_dir(&stencils_dir)
        .args([
            "rustc",
            "--release",
            "--lib",
            "--target-dir",
            target_dir.to_str().unwrap(),
            "--",
            "-C",
            "relocation-model=static",
        ])
        .status()?;
    if !status.success() {
        return Err("failed to compile stencils crate".into());
    }

    println!("cargo:rerun-if-changed={}/src", stencils_dir.display());

    let rlib = target_dir
        .join("release")
        .join(dir_to_libname(&stencils_dir)?);
    let extraction = extract::extract(&rlib)?;
    generate::generate(extraction, &out_path)
}

fn dir_to_libname(rel: &Path) -> Result<String, Box<dyn Error>> {
    let manifest = fs::read_to_string(rel.join("Cargo.toml"))?;
    let name = manifest
        .lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix("name = ")
                .map(|value| value.trim_matches('"'))
        })
        .ok_or("package name not found in stencils Cargo.toml")?;
    Ok(format!("lib{}.rlib", name.replace("-", "_")))
}

fn find_target_dir(out_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let profile = std::env::var("PROFILE")?;
    for parent in out_dir.ancestors() {
        if parent.ends_with(&profile) {
            return Ok(parent
                .parent()
                .ok_or("failed to find target dir")?
                .to_path_buf());
        }
    }
    Err("failed to find target dir".into())
}
