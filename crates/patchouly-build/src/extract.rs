use std::{
    collections::{BTreeMap, HashMap, hash_map::Entry},
    error::Error,
    fs::File,
    io::{Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
};

use memmap2::Mmap;
use object::{
    Object, ObjectSection, ObjectSymbol, Relocation, RelocationEncoding, RelocationKind,
    RelocationTarget, Section, Symbol, SymbolKind, read::archive::ArchiveFile,
};
use patchouly_core::{
    Stencil, StencilFamilyBuild,
    relocation::{
        PatchKind, Relocation as StencilRelocation, RelocationEncoding as StencilRelocationEncoding,
    },
    stencils::{io_to_index, stencils_len},
};
use smallvec::SmallVec;

use crate::structs::StencilArgs;

const LIB_NAME_SYMBOL: &[u8] = b"__STENCIL_API_NAME";
const STENCIL_FUNC_PREFIX: &str = "__patchouly__";

enum FileContents {
    Mmap(Mmap),
    Vec(Vec<u8>),
}
impl FileContents {
    fn open(path: &Path) -> Result<FileContents, Box<dyn Error>> {
        let mut file = File::open(path)?;
        if let Ok(mmap) = unsafe { Mmap::map(&file) } {
            Ok(FileContents::Mmap(mmap))
        } else {
            let mut ar_data = Vec::with_capacity(file.metadata()?.len() as usize);
            file.seek(SeekFrom::Start(0))?;
            file.read_to_end(&mut ar_data)?;
            Ok(FileContents::Vec(ar_data))
        }
    }
    fn as_slice(&self) -> &[u8] {
        match self {
            FileContents::Mmap(mmap) => &mmap[..],
            FileContents::Vec(data) => data.as_slice(),
        }
    }
}

struct StencilFamilyBuilder {
    family: StencilFamilyBuild,
    existing_relocations: HashMap<SmallVec<[StencilRelocation; 8]>, usize>,
}
impl StencilFamilyBuilder {
    fn new(metadata: Metadata) -> Self {
        let mut family = StencilFamilyBuild {
            IN: metadata.inputs as usize,
            OUT: metadata.outputs as usize,
            MAX_REGS: metadata.max_regs as usize,
            HOLES: metadata.holes as usize,
            JUMPS: metadata.jumps as usize,
            relocation_data: Default::default(),
            stencils: vec![],
        };
        let new_len = stencils_len(family.IN, family.OUT, family.MAX_REGS);
        family.stencils.resize(new_len, Default::default());
        Self {
            family,
            existing_relocations: Default::default(),
        }
    }

    fn add_stencil(
        &mut self,
        code: Range<usize>,
        io: StencilArgs,
        stencil: SmallVec<[StencilRelocation; 8]>,
    ) -> Result<(), Box<dyn Error>> {
        let holes = stencil
            .iter()
            .filter_map(|reloc| {
                if let PatchKind::Hole = reloc.patch_kind() {
                    Some(reloc.patch_id() + 1)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0) as usize;
        let jumps = stencil
            .iter()
            .filter_map(|reloc| {
                if let PatchKind::Target = reloc.patch_kind() {
                    Some(reloc.patch_id() + 1)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0) as usize;

        if self.family.IN != io.inputs.len()
            || self.family.OUT != io.outputs.len()
            || self.family.HOLES != holes
            || self.family.JUMPS != jumps
        {
            return Err("Stencil family mismatch".into());
        }

        let relocation_start = match self.existing_relocations.entry(stencil) {
            Entry::Occupied(occupied_entry) => *occupied_entry.get(),
            Entry::Vacant(vacant_entry) => {
                let start = self.family.relocation_data.len();
                let data = &mut self.family.relocation_data;
                data.extend(vacant_entry.key());
                data.push(StencilRelocation::new());
                vacant_entry.insert(start);
                start
            }
        };

        let stencil = Stencil {
            code_index: code.start.try_into()?,
            code_len: code.len().try_into()?,
            relocation_index: relocation_start.try_into()?,
        };
        let index = io_to_index(&io.inputs, &io.outputs, self.family.MAX_REGS);
        self.family.stencils[index] = stencil;

        Ok(())
    }

    fn finalize(self) -> StencilFamilyBuild {
        self.family
    }
}

pub struct Extraction {
    pub lib_name: String,
    pub all_code: Vec<u8>,
    pub max_regs: usize,
    pub families: BTreeMap<String, StencilFamilyBuild>,
}
pub fn extract(rlib_path: &Path) -> Result<Extraction, Box<dyn Error>> {
    let rlib_file = FileContents::open(rlib_path)?;
    let ar_data = rlib_file.as_slice();
    let ar = ArchiveFile::parse(ar_data)?;

    let mut lib_name = None;
    let mut max_regs = None;
    let mut all_code = Vec::with_capacity(ar_data.len() / 2);
    let mut stencils = HashMap::new();

    for entry in ar.members() {
        let entry = entry?;
        let name = entry.name();
        if !name.ends_with(b".o") {
            continue;
        }
        let data = entry.data(ar_data)?;
        let file = object::File::parse(data)?;

        'next: for symbol in file.symbols() {
            let kind = symbol.kind();
            if kind != SymbolKind::Text {
                if kind == SymbolKind::Data
                    && let Ok(LIB_NAME_SYMBOL) = symbol.name_bytes()
                    && let Some((_, data)) = get_data(&file, &symbol)
                {
                    lib_name = Some(String::from_utf8_lossy(data).to_string());
                }
                continue;
            }

            let sym_name = if let Ok(name) = symbol.name()
                && name.starts_with(STENCIL_FUNC_PREFIX)
            {
                &name[STENCIL_FUNC_PREFIX.len()..]
            } else {
                continue;
            };
            let Some((name, io)) = parse_name(sym_name) else {
                continue;
            };
            let Some((section, code)) = get_data(&file, &symbol) else {
                continue;
            };

            let mut relocations = SmallVec::<[_; 8]>::new();
            for (offset, info) in section.relocations() {
                let RelocationTarget::Symbol(target) = info.target() else {
                    continue 'next;
                };
                let Ok(sym) = file.symbol_by_index(target) else {
                    continue 'next;
                };
                if sym.section_index().is_some() {
                    // relocation to a defined function/data
                    return Err(format!(
                        "all function calls/data in stencils must be inlined: {}",
                        name,
                    )
                    .into());
                }

                let Some(reloc) = sym.name().ok() else {
                    continue 'next;
                };
                let Some(patch) = get_patch_type(name, reloc) else {
                    continue 'next;
                };
                let reloc = new_relocation(offset, &info, patch)?;
                relocations.push(reloc);
            }

            let start = all_code.len();
            all_code.extend_from_slice(code);

            let mut anchor = None;
            match stencils.entry(name) {
                Entry::Occupied(occupied_entry) => anchor.get_or_insert(occupied_entry).get_mut(),
                Entry::Vacant(vacant_entry) => {
                    let metadata = file
                        .symbol_by_name(&format!("{}{}__meta", STENCIL_FUNC_PREFIX, name))
                        .and_then(|symbol| get_data(&file, &symbol))
                        .and_then(|(_, data)| Metadata::unpack(data));
                    let Some(metadata) = metadata else {
                        return Err(format!("no meta symbol for {}", name).into());
                    };
                    if let Some(max_regs) = max_regs {
                        if max_regs != metadata.max_regs {
                            return Err(format!(
                                "all stencils must have the same max_regs: {}",
                                name,
                            )
                            .into());
                        }
                    } else {
                        max_regs = Some(metadata.max_regs);
                    }
                    let builder = StencilFamilyBuilder::new(metadata);
                    vacant_entry.insert(builder)
                }
            }
            .add_stencil(start..all_code.len(), io, relocations)?;
        }
    }

    Ok(Extraction {
        lib_name: lib_name.unwrap(),
        all_code,
        max_regs: max_regs.unwrap_or(0) as usize,
        families: stencils
            .into_iter()
            .map(|(name, builder)| (name.to_string(), builder.finalize()))
            .collect(),
    })
}

fn get_data<'file>(
    file: &'file object::File<'file>,
    symbol: &Symbol,
) -> Option<(Section<'file, 'file>, &'file [u8])> {
    let section = symbol.section_index()?;
    let section = file.section_by_index(section).ok()?;
    section
        .data_range(symbol.address(), symbol.size())
        .ok()
        .flatten()
        .map(|data| (section, data))
}

#[derive(Debug, Clone, Copy)]
struct Metadata {
    inputs: u16,
    outputs: u16,
    max_regs: u16,
    holes: u16,
    jumps: u16,
}
impl Metadata {
    /// See `stencil.rs` in `patchouly-macros`
    fn unpack(data: &[u8]) -> Option<Self> {
        if data.len() != 10 {
            return None;
        }
        Some(Metadata {
            inputs: u16::from_le_bytes(data[0..2].try_into().unwrap()),
            outputs: u16::from_le_bytes(data[2..4].try_into().unwrap()),
            max_regs: u16::from_le_bytes(data[4..6].try_into().unwrap()),
            holes: u16::from_le_bytes(data[6..8].try_into().unwrap()),
            jumps: u16::from_le_bytes(data[8..10].try_into().unwrap()),
        })
    }
}

fn parse_name(sym_name: &str) -> Option<(&str, StencilArgs)> {
    if sym_name == "__empty____" {
        return Some(("__empty", StencilArgs::default()));
    }
    let mut split = sym_name.split("__");
    let name = split.next()?;
    let inputs = split.next()?;
    let outputs = split.next()?;
    if split.next().is_none() {
        Some((name, StencilArgs::parse(inputs, outputs)?))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
enum PatchType {
    Hole(u16),
    Stack(u16),
    Target(u16),
    Next,
}

fn get_patch_type(name: &str, reloc: &str) -> Option<PatchType> {
    if reloc == "copy_and_patch_next" {
        return Some(PatchType::Next);
    }
    assert!(reloc.starts_with(name), "reloc: {reloc}, name: {name}");
    let name = &reloc[name.len()..];
    assert!(name.starts_with("__"));
    let name = &name[2..];

    /// Parse ints with trailing chars
    fn parse_int(s: &str) -> Option<u16> {
        let end = s
            .as_bytes()
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(s.len());
        s[..end].parse().ok()
    }

    if let Some(name) = name.strip_prefix("stack") {
        parse_int(name).map(PatchType::Stack)
    } else if let Some(name) = name.strip_prefix("target") {
        parse_int(name).map(PatchType::Target)
    } else if let Some(name) = name.strip_prefix("hole") {
        parse_int(name).map(PatchType::Hole)
    } else {
        None
    }
}

fn new_relocation(
    offset: u64,
    relocation: &Relocation,
    patch: PatchType,
) -> Result<StencilRelocation, Box<dyn Error>> {
    let mut reloc = StencilRelocation::new();

    let offset: u16 = offset.try_into()?;
    reloc.checked_set_offset(offset)?;

    let encoding = match relocation.encoding() {
        RelocationEncoding::Generic => StencilRelocationEncoding::Generic,
        RelocationEncoding::X86Signed => StencilRelocationEncoding::X86Signed,
        _ => return Err("unsupported relocation encoding".into()),
    };
    reloc.checked_set_encoding(encoding)?;

    let relative = match relocation.kind() {
        RelocationKind::Absolute => false,
        RelocationKind::Relative | RelocationKind::PltRelative => true,
        _ => return Err("unsupported relocation kind".into()),
    };
    reloc.checked_set_relative(relative)?;

    let size = relocation.size();
    reloc.checked_set_size(size)?;

    let addend = relocation.addend().try_into()?;
    reloc.checked_set_addend(addend)?;

    let (kind, extra) = match patch {
        PatchType::Hole(i) => (PatchKind::Hole, i),
        PatchType::Stack(i) => (PatchKind::Stack, i),
        PatchType::Target(i) => (PatchKind::Target, i),
        PatchType::Next => (PatchKind::Target, 0),
    };
    reloc.checked_set_patch_kind(kind)?;
    reloc.checked_set_patch_id(extra)?;

    Ok(reloc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name() {
        let max_args = 10;
        let assertions = [
            ("add_const__1__0", "add_const", 10),
            ("add_const__9__0", "add_const", 90),
        ];
        for (input, name, index) in assertions {
            let (n, args) = parse_name(input).unwrap();
            assert_eq!(n, name);
            assert_eq!(io_to_index(&args.inputs, &args.outputs, max_args), index);
        }
    }
}
