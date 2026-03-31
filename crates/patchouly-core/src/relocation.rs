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
            unsafe { core::mem::transmute::<u8, Self>(bits) }
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
            unsafe { core::mem::transmute::<u8, Self>(bits) }
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

    pub fn apply_raw(&self, dest: &mut [u8], value: usize) {
        let value = value.wrapping_add_signed(self.addend() as isize);
        let offset = self.offset();
        let size = self.size();
        match self.encoding() {
            RelocationEncoding::Generic => {
                let size = (size / 8) as usize;
                dest[offset as usize..][..size].copy_from_slice(&value.to_le_bytes()[..size]);
            }
            RelocationEncoding::X86Signed => {
                let size = (size / 8) as usize;
                dest[offset as usize..][..size].copy_from_slice(&value.to_le_bytes()[..size]);
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Copy, Clone)]
pub enum JumpTarget {
    /// To the next stencil
    Next,
    /// TODO: To a specific block id
    Target(u16),
}

pub struct DelayedRelocation {
    offset: usize,
    relocation: Relocation,
    target: u16,
}
impl DelayedRelocation {
    pub fn try_apply(
        dest: &mut [u8],
        offset: usize,
        relocation: Relocation,
        stack_vars: &[usize],
        holes: &[usize],
        jumps: &[JumpTarget],
    ) -> Option<Self> {
        let value = match relocation.patch_info().unwrap() {
            // TODO: unwrap safety?
            PatchInfo::Hole(i) => holes[i as usize],
            PatchInfo::Stack(i) => stack_vars[i as usize],
            PatchInfo::Target(i) => match jumps[i as usize] {
                // jump to the end
                JumpTarget::Next => dest.len() - offset - relocation.offset() as usize,
                // delayed
                JumpTarget::Target(target) => {
                    return Some(DelayedRelocation {
                        offset,
                        relocation,
                        target,
                    });
                }
            },
        };

        relocation.apply_raw(&mut dest[offset..], value);
        None
    }

    pub fn target(&self) -> u16 {
        self.target
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn apply(&self, dest: &mut [u8], value: isize) {
        self.relocation
            .apply_raw(&mut dest[self.offset..], value as usize);
    }
}
