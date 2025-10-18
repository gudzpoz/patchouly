use object::read::archive::ArchiveFile;
use object::{Object, ObjectSection, ObjectSymbol, Relocation, RelocationTarget, SymbolKind};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::{path::Path, process::Command};
use std::error::Error;
use std::fs::File;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = std::env::var("OUT_DIR")?;
    let out_path = Path::new(&out_dir);
    let current_dir = std::env::current_dir()?;
    let stencils_dir = current_dir.as_path().parent().ok_or("wrong pwd")?.join("stencils");

    Command::new("cargo")
        .current_dir(&stencils_dir)
        // note the "relocation-model=static" here: it changes relocation types,
        //   with it being pic, things are more complicated so we will use static.
        .args(["rustc", "--release", "--lib", "--", "-C", "relocation-model=static"])
        .status().unwrap();
    println!("cargo::rerun-if-changed={}/src/lib.rs", stencils_dir.display());

    // .rlib files are internal compilation product...
    // however, currently using .a (with crate-type=staticlib) yields a 20MB huge blob,
    // so we use .rlib here (which sizes ~20kB) for simplicity.
    let rlib = find_release_compilation("libstencils.rlib", out_path).ok_or("stencil rlib not found")?;
    let stencils = extract_stencils(rlib)?;
    let (code, (bin_file, binary)) = generate_emits(&stencils);

    let bin_path = out_path.join(bin_file);
    let mut bin_file = File::options().write(true).create(true).truncate(true).open(bin_path)?;
    bin_file.write_all(&binary)?;
    let code_path = out_path.join("stencils.rs");
    let mut rust_file = File::options().write(true).create(true).truncate(true).open(code_path)?;
    rust_file.write_all(code.as_bytes())?;

    Ok(())
}

struct Stencil {
    name: String,
    code: Vec<u8>,
    holes: Vec<(u64, String, Relocation)>,
    jumps: Vec<(u64, String, Relocation)>,
}
type Stencils = HashMap<String, Stencil>;
const JUMP_RELOCATION_PREFIX: &str = "copy_and_patch_next";
const HOLE_RELOCATION_PREFIX: &str = "HOLE";

fn find_release_compilation(name: &str, cwd: &Path) -> Option<PathBuf> {
    let full = cwd.canonicalize().ok()?;
    let mut dir = full.as_path();
    loop {
        let file = dir.join("release").join(name);
        if file.exists() {
            return Some(file);
        }
        if let Some(parent) = dir.parent() {
            dir = parent;
            continue;
        }
        return None;
    }
}

fn extract_stencils(rlib: PathBuf) -> Result<Stencils, Box<dyn Error>> {
    let mut ar_data = Vec::new();
    File::open(&rlib)?.read_to_end(&mut ar_data)?;
    let archive = ArchiveFile::parse(&*ar_data)?;
    let mut stencils = HashMap::new();
    for entry in archive.members() {
        let entry = entry?;
        let name = String::from_utf8_lossy(entry.name());
        if !name.ends_with(".o") {
            continue;
        }
        let data = entry.data(&*ar_data)?;
        let file = object::File::parse(data)?;

        for symbol in file.symbols() {
            if symbol.kind() != SymbolKind::Text {
                continue;
            }

            let name = symbol.name()?.to_string();

            let Some(section) = symbol.section_index() else { continue };
            let section = file.section_by_index(section)?;
            let code = section.data_range(symbol.address(), symbol.size())?.unwrap().into();

            let relocations: Vec<_> = section.relocations()
                .map(|(offset, info)| {
                    let RelocationTarget::Symbol(target) = info.target() else {
                        panic!("unsupported relocation");
                    };
                    let target = file.symbol_by_index(target).unwrap().name().unwrap();
                    (offset, target.to_string(), info)
                })
                .collect();
            let mut holes = Vec::new();
            let mut jumps = Vec::new();
            for reloc in relocations {
                if reloc.1.starts_with(HOLE_RELOCATION_PREFIX) {
                    holes.push(reloc);
                } else if reloc.1.starts_with(JUMP_RELOCATION_PREFIX) {
                    jumps.push(reloc);
                } else {
                    panic!("unknown relocation target: {}", reloc.1);
                }
            }


            let stencil = Stencil { name, code, holes, jumps };
            stencils.insert(stencil.name.clone(), stencil);
        }
    }
    Ok(stencils)
}

fn generate_emits(stencils: &Stencils) -> (String, (&'static str, Vec<u8>)) {
    let mut bytes = Vec::new();
    let mut rust = r#"
pub struct TargetId(pub isize);
pub struct PtrRelocation {
    pub offset: usize,
    pub target_id: TargetId,
    pub size: usize,
}
"#.to_string();
    for (name, stencil) in stencils.iter() {
        let code_start = bytes.len();
        bytes.extend_from_slice(&stencil.code);
        let code_end = bytes.len();

        let mut extra_args = stencil.holes.iter()
            .map(|(_, hole, _)| format!(", {}: usize", hole.to_lowercase()))
            .collect::<Vec<_>>();
        extra_args.extend(stencil.jumps.iter().map(
            |(_, jump, _)| format!(", {}: TargetId", name_to_target_id_param(jump)),
        ));
        let extra_args = extra_args.join("");
        let return_type = std::iter::repeat_n("PtrRelocation", stencil.jumps.len())
            .collect::<Vec<_>>().join(", ");
        let extra_header = if stencil.holes.is_empty() && stencil.jumps.is_empty() {
            ""
        } else {
            "\n    let base = buf.len();"
        };

        let hole_patching = stencil.holes.iter().map(|(offset, hole, info)| {
            if info.size() != 32 {
                panic!("unsupported relocation size: {}", info.size());
            }
            format!(
                "
    buf[base+{}..base+{}+4].copy_from_slice(&({} as u32).to_ne_bytes());",
                    offset, offset, hole.to_lowercase(),
            )
        }).collect::<Vec<_>>().join("");

        let return_values = stencil.jumps.iter().map(|(offset, name, info)| {
            format!(
                "PtrRelocation {{ offset: base + {}, target_id: {}, size: {} }}",
                *offset, name_to_target_id_param(name), info.size(),
            )
        }).collect::<Vec<_>>().join(", ");

        rust.push_str(
            &format!("
#[allow(unused_parens)]
pub fn emit_{}(buf: &mut Vec<u8>{}) -> ({}) {{{}
    buf.extend_from_slice(&CODE_BYTES[{}..{}]);{}
    ({})
}}
",
            name, extra_args, return_type, extra_header,
            code_start, code_end, hole_patching,
            return_values,
        ));
    }
    rust.insert_str(0, &format!(
        r#"
const CODE_BYTES: &[u8; {}] = include_bytes!("libstencils.bin");
"#, bytes.len(),
    ));
    (rust, ("libstencils.bin", bytes))
}

fn name_to_target_id_param(name: &str) -> &str {
    if name == JUMP_RELOCATION_PREFIX {
        "next"
    } else {
        &name[JUMP_RELOCATION_PREFIX.len()..]
    }
}
