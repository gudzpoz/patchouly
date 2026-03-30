use bitfields::bitfield;

/// A relocation entry
///
/// It roughly follows the `Relocation` struct from the `object` crate,
/// with some simplifications.
#[bitfield(u64)]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Relocation {
    /// The address where relocation is required
    ///
    /// Note that we think `u16` is good enough for a stencil:
    /// if your stencils go beyond 10k, you're probably doing something wrong.
    offset: u16,
    /// Encoding, how a relocation should be written in the place.
    #[bits(7)]
    encoding: RelocationEncoding,
    /// Whether the relocation is relative
    ///
    /// `RelocationKind::Absolute/Relative/PltRelative` in the `object` crate.
    /// Most often, this is `true` for jump target, and `false` for values.
    relative: bool,
    /// Size in bits
    ///
    /// Usually one just can't fit a `usize` into a relocation. Users might
    /// need to use multiple holes for that and we will warn them using this
    /// field.
    size: u8,
    /// Addend, extra value to add to the target before putting it in place.
    ///
    /// Again, we assume `i12` is enough here.
    #[bits(12)]
    addend: i16,
    /// What this relocation is for
    #[bits(4)]
    patch_kind: PatchKind,
    /// See [PatchInfo]
    patch_id: u16,
}

#[derive(Copy, Clone)]
#[repr(u8)]
pub enum RelocationEncoding {
    Invalid = 0,
    /// Plain value
    Generic,
    /// Sign extended
    X86Signed,
    /// Upper limit
    Unknown,
}
impl RelocationEncoding {
    const fn from_bits(bits: u8) -> Self {
        if bits >= Self::Unknown as u8 {
            Self::Invalid
        } else {
            unsafe { std::mem::transmute::<u8, Self>(bits) }
        }
    }
    const fn into_bits(self) -> u8 {
        self as u8
    }
}

#[derive(Copy, Clone)]
#[repr(u8)]
pub enum PatchKind {
    Hole = 0,
    Stack,
    Target,
    Unknown,
}
impl PatchKind {
    const fn from_bits(bits: u8) -> Self {
        if bits >= Self::Unknown as u8 {
            Self::Unknown
        } else {
            unsafe { std::mem::transmute::<u8, Self>(bits) }
        }
    }
    const fn into_bits(self) -> u8 {
        self as u8
    }
}

pub enum PatchInfo {
    Hole(u16),
    Stack(u16),
    Target(u16),
}

impl Relocation {
    pub fn is_invalid(&self) -> bool {
        matches!(self.encoding(), RelocationEncoding::Invalid)
    }

    pub fn patch_info(&self) -> Option<PatchInfo> {
        Some(match self.patch_kind() {
            PatchKind::Hole => PatchInfo::Hole(self.patch_id()),
            PatchKind::Stack => PatchInfo::Stack(self.patch_id()),
            PatchKind::Target => PatchInfo::Target(self.patch_id()),
            _ => return None,
        })
    }

    pub fn apply(&self, dest: &mut [u8], stack_vars: &[usize], holes: &[usize], jumps: &[usize]) {
        let value = match self.patch_info().unwrap() {
            PatchInfo::Hole(i) => holes[i as usize],
            PatchInfo::Stack(i) => stack_vars[i as usize],
            PatchInfo::Target(i) => jumps[i as usize],
        };

        let value = value.wrapping_add_signed(self.addend() as isize);

        match self.encoding() {
            RelocationEncoding::Generic => {
                let size = (self.size() / 8) as usize;
                dest[self.offset() as usize..][..size]
                    .copy_from_slice(&value.to_le_bytes()[..size]);
            }
            RelocationEncoding::X86Signed => {
                let size = (self.size() / 8) as usize;
                dest[self.offset() as usize..][..size]
                    .copy_from_slice(&value.to_le_bytes()[..size]);
            }
            _ => unreachable!(),
        }
    }
}
