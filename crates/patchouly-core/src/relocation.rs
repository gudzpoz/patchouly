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
    Runtime,
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
    Runtime(u16),
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
            PatchKind::Runtime => PatchInfo::Runtime(self.patch_id()),
            _ => return None,
        })
    }

    pub fn supports_value(&self, value: usize) -> bool {
        if self.relative() {
            return false;
        }
        match self.encoding() {
            RelocationEncoding::Generic => fits_unsigned(value, self.size()),
            RelocationEncoding::X86Signed => fits_signed(value as isize, self.size()),
            _ => false,
        }
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

fn fits_unsigned(value: usize, bits: u8) -> bool {
    match bits {
        0 => value == 0,
        bits if bits as u32 >= usize::BITS => true,
        bits => value < (1usize << bits),
    }
}

fn fits_signed(value: isize, bits: u8) -> bool {
    match bits {
        0 => value == 0,
        bits if bits as u32 >= isize::BITS => true,
        bits => {
            let shift = isize::BITS - bits as u32;
            (value << shift >> shift) == value
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

#[derive(Copy, Clone)]
pub enum DelayedTarget {
    Block(u16),
    Constant(u16),
    Next,
}

#[derive(Clone)]
pub struct DelayedRelocation {
    offset: usize,
    relocation: Relocation,
    target: DelayedTarget,
}
impl DelayedRelocation {
    pub fn try_apply(
        dest: &mut [u8],
        offset: usize,
        relocation: Relocation,
        stack_vars: &[usize],
        holes: &[usize],
        jumps: &[JumpTarget],
        rt_symbols: &[unsafe fn()],
    ) -> Option<Self> {
        // TODO: unwrap safety?
        let mut value = match relocation.patch_info().unwrap() {
            PatchInfo::Hole(i) => holes[i as usize],
            PatchInfo::Stack(i) => stack_vars[i as usize],
            PatchInfo::Target(i) => {
                return Some(DelayedRelocation {
                    offset,
                    relocation,
                    target: match jumps[i as usize] {
                        JumpTarget::Next => DelayedTarget::Next,
                        JumpTarget::Target(target) => DelayedTarget::Block(target),
                    },
                });
            }
            PatchInfo::Runtime(i) => {
                rt_symbols[i as usize] as usize
            }
        };

        if relocation.relative() {
            value -= offset + relocation.offset() as usize
        }

        relocation.apply_raw(&mut dest[offset..], value);
        None
    }

    pub fn constant(offset: usize, relocation: Relocation, constant: u16) -> Self {
        Self {
            offset,
            relocation,
            target: DelayedTarget::Constant(constant),
        }
    }

    pub fn next(offset: usize, relocation: Relocation) -> Self {
        Self {
            offset,
            relocation,
            target: DelayedTarget::Next,
        }
    }

    pub fn target(&self) -> DelayedTarget {
        self.target
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn location(&self) -> usize {
        self.offset + self.relocation.offset() as usize
    }

    pub fn resolve(&self, base: usize, value: usize) -> usize {
        if self.relocation.relative() {
            value.wrapping_sub(base + self.location())
        } else {
            value
        }
    }

    pub fn apply(&self, dest: &mut [u8], value: usize) {
        self.relocation.apply_raw(&mut dest[self.offset..], value);
    }
}
