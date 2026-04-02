pub mod extract;
pub mod generate;
mod structs;

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

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
/// `setup_stencils!("...");` macro call in the stencils crate.
/// For example, `setup_stencils!("Calc");` will generate a file
/// named `calc_stencils.rs`, which you may include with:
///
/// ```ignore
/// include!(concat!(env!("OUT_DIR"), "/calc_stencils.rs"));
/// ```
pub fn extract(rel_stencils_dir: &str) -> Result<(), Box<dyn Error>> {
    let out_dir = std::env::var("OUT_DIR")?;
    let out_path = Path::new(&out_dir).canonicalize()?;
    // We need a different target dir to prevent deadlock
    let target_dir = find_target_dir(&out_path)?.join("patchouly");
    let current_dir = std::env::current_dir()?;
    let stencils_dir = current_dir
        .as_path()
        .parent()
        .ok_or("expected to be in a workspace")?
        .join(rel_stencils_dir)
        .canonicalize()?;
    assert!(
        stencils_dir.exists(),
        "stencils dir {} does not exist",
        stencils_dir.display()
    );

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

    let rlib = target_dir.join("release").join(dir_to_libname(&stencils_dir)?);
    let extraction = extract::extract(&rlib)?;
    generate::generate(extraction, &out_path)?;

    Ok(())
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
            return Ok(parent.parent().ok_or("failed to find target dir")?.to_path_buf());
        }
    }
    Err("failed to find target dir".into())
}
