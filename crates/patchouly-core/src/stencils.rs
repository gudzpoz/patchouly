use crate::relocation::{PatchInfo, Relocation};

/// Describes the way to pass a variable in and out of a stencil
pub struct StencilFamily<
    const IN: usize,
    const OUT: usize,
    const MAX_REGS: usize,
    const HOLES: usize,
    const JUMPS: usize,
> {
    /// Pool of stencil relocation data, possibly shared by multiple stencils
    ///
    /// The relocation data for each stencil is sorted by relocation offset.
    pub relocation_data: &'static [Relocation],
    pub stencils: &'static [Stencil<IN, OUT, HOLES, JUMPS>],
}

#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct Stencil<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize> {
    pub code_index: u32,
    pub code_len: u16,
    /// Points to the first relocation in [StencilFamily::relocation_data]
    ///
    /// The end of the relocations are marked by a
    /// `Relocation { encoding: Invalid }` (all zeroes) entry.
    pub relocation_index: u16,
}
pub type UntypedStencil = Stencil<0, 0, 0, 0>;

#[derive(Clone, Copy, Debug)]
pub enum Location {
    Stack(u16),
    Register(u16),
}
impl Location {
    #[doc(hidden)]
    pub fn from_bits(bits: u16) -> Self {
        match bits {
            0 => Location::Stack(0),
            _ => Location::Register(bits - 1),
        }
    }

    #[doc(hidden)]
    pub fn into_bits(&self) -> u16 {
        match self {
            Location::Stack(_) => 0,
            Location::Register(i) => i + 1,
        }
    }
}

impl<
    const IN: usize,
    const OUT: usize,
    const MAX_REGS: usize,
    const HOLES: usize,
    const JUMPS: usize,
> StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>
{
    pub fn inputs(&self) -> usize {
        IN
    }
    pub fn outputs(&self) -> usize {
        OUT
    }
    pub fn max_regs(&self) -> usize {
        MAX_REGS
    }
    pub fn holes(&self) -> usize {
        HOLES
    }
    pub fn jumps(&self) -> usize {
        JUMPS
    }

    pub fn select(
        &self,
        inputs: &[Location; IN],
        outputs: &[Location; OUT],
        holes: &[usize; HOLES],
    ) -> (bool, &Stencil<IN, OUT, HOLES, JUMPS>) {
        let index = io_to_index(inputs, outputs, MAX_REGS, false);
        let stencil = &self.stencils[index];
        for reloc in stencil.relocations(self) {
            if reloc.is_invalid() {
                return (false, stencil);
            }
            if let Some(PatchInfo::Hole(i)) = reloc.patch_info()
                && !reloc.supports_value(holes[i as usize])
            {
                break;
            }
        }
        (
            true,
            &self.stencils[index + stencils_len(IN, OUT, MAX_REGS)],
        )
    }
}

#[doc(hidden)]
pub fn io_to_index(
    inputs: &[Location],
    outputs: &[Location],
    max_regs: usize,
    wide: bool,
) -> usize {
    let mut i = 0;
    for var in inputs.iter().chain(outputs.iter()) {
        i = max_regs * i
            + match var {
                Location::Stack(_) => 0,
                Location::Register(i) => *i as usize + 1,
            };
    }
    i + if wide {
        stencils_len(inputs.len(), outputs.len(), max_regs)
    } else {
        0
    }
}

#[doc(hidden)]
pub fn index_to_io_lossy(
    index: usize,
    max_regs: usize,
    inputs: &mut [Location],
    outputs: &mut [Location],
) -> bool {
    fn process_index(mut index: usize, max_regs: usize, slots: &mut [Location]) -> usize {
        for v in slots.iter_mut().rev() {
            let reg = index % max_regs;
            index = (index - reg) / max_regs;
            *v = if reg == 0 {
                Location::Stack(0)
            } else {
                Location::Register(reg as u16 - 1)
            };
        }
        index
    }

    let index = process_index(index, max_regs, outputs);
    let index = process_index(index, max_regs, inputs);
    assert!(index == 0 || index == 1);
    index == 1
}

#[doc(hidden)]
pub const fn stencils_len(inputs: usize, outputs: usize, max_regs: usize) -> usize {
    max_regs.pow(inputs as u32 + outputs as u32)
}

impl<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>
    Stencil<IN, OUT, HOLES, JUMPS>
{
    #[doc(hidden)]
    pub const fn from_bits(bits: u64) -> Self {
        Stencil {
            code_index: (bits & 0xFFFFFFFF) as u32,
            code_len: ((bits >> 32) & 0xFFFF) as u16,
            relocation_index: ((bits >> 48) & 0xFFFF) as u16,
        }
    }

    #[doc(hidden)]
    pub const fn into_bits(self) -> u64 {
        (self.code_index as u64)
            | ((self.code_len as u64) << 32)
            | ((self.relocation_index as u64) << 48)
    }

    pub fn code<'a>(&self, store: &'a [u8]) -> &'a [u8] {
        &store[self.code_index as usize..self.code_index as usize + self.code_len as usize]
    }

    pub fn relocations<'a, const MAX_REGS: usize>(
        &self,
        store: &'a StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
    ) -> &'a [Relocation] {
        &store.relocation_data[self.relocation_index as usize..]
    }

    pub fn untyped(&self) -> UntypedStencil {
        UntypedStencil::from_bits(self.into_bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestStencil = Stencil<0, 0, 0, 0>;

    #[test]
    fn test_stencil_bits() {
        let mut i = 1u64;
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            assert_eq!(TestStencil::from_bits(i).into_bits(), i);
        }
    }

    #[test]
    fn test_io_to_index() {
        let mut i = 1usize;
        let len = stencils_len(4, 4, 10);
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            let mut inputs = [Location::Stack(0); 4];
            let mut outputs = [Location::Stack(0); 4];
            index_to_io_lossy(i % len, 10, &mut inputs, &mut outputs);
            assert_eq!(io_to_index(&inputs, &outputs, 10, false), i % len);
        }
    }
}
