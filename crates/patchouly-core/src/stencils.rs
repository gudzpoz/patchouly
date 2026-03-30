use crate::relocation::Relocation;

/// Defines structs for stencils
///
/// ## Internals
///
/// The `patchouly-build` crate will generate Rust code
/// that instantiates these structs.
/// To help keeping things in sync, we use this macro to
/// define two kinds of structs: one for the users (in the generated code)
/// and one for `patchouly-build` for serialization.
/// (The reason that we can't use a single struct for both is the templates.)
macro_rules! define_struct {
    ($(#[$outer:meta])*
    struct ($name:ident | $name_build:ident)<$(const $tmpl:ident: $tmpl_ty:ty),*> {
        $($(#[$inner:meta])* pub $input:ident: $input_ty:ty | $input_tmpl_ty:ty),* $(,)?
    }) => {
        $(#[$outer])*
        pub struct $name<$(const $tmpl: $tmpl_ty),*> {
            $($(#[$inner])* pub $input: $input_ty),*,
        }

        /// Used by `patchouly-build`
        #[allow(non_snake_case)]
        #[doc(hidden)]
        pub struct $name_build {
            $(pub $tmpl: $tmpl_ty),*,
            $(pub $input: $input_tmpl_ty),*,
        }
    };
}

define_struct!(
    /// Describes the way to pass a variable in and out of a stencil
    struct (StencilFamily | StencilFamilyBuild)<
        const IN: usize, const OUT: usize, const MAX_REGS: usize,
        const HOLES: usize, const JUMPS: usize
    > {
        /// Pool of stencil relocation data, possibly shared by multiple stencils
        pub relocation_data: &'static [Relocation] | Vec<Relocation>,
        pub stencils: &'static [Stencil<IN, OUT, HOLES, JUMPS>] | Vec<Stencil<0, 0, 0, 0>>,
    }
);

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

#[derive(Clone, Copy, Debug)]
pub enum Variable {
    Stack(u16),
    Register(u16),
}
impl Variable {
    #[doc(hidden)]
    pub fn from_bits(bits: u16) -> Self {
        match bits {
            0 => Variable::Stack(0),
            _ => Variable::Register(bits - 1),
        }
    }

    #[doc(hidden)]
    pub fn into_bits(&self) -> u16 {
        match self {
            Variable::Stack(_) => 0,
            Variable::Register(i) => i + 1,
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
        inputs: &[Variable; IN],
        outputs: &[Variable; OUT],
    ) -> &Stencil<IN, OUT, HOLES, JUMPS> {
        &self.stencils[io_to_index(inputs, outputs, MAX_REGS)]
    }
}

#[doc(hidden)]
pub fn io_to_index(inputs: &[Variable], outputs: &[Variable], max_regs: usize) -> usize {
    let mut i = 0;
    for var in inputs.iter().chain(outputs.iter()) {
        i = max_regs * i
            + match var {
                Variable::Stack(_) => 0,
                Variable::Register(i) => *i as usize + 1,
            };
    }
    i
}

#[doc(hidden)]
pub fn index_to_io_lossy(
    mut index: usize,
    inputs: usize,
    outputs: usize,
    max_regs: usize,
) -> (Vec<Variable>, Vec<Variable>) {
    let mut all = Vec::new();
    for _ in 0..(inputs + outputs) {
        let reg = index % max_regs;
        index = (index - reg) / max_regs;
        if reg == 0 {
            all.push(Variable::Stack(0));
        } else {
            all.push(Variable::Register(reg as u16 - 1));
        }
    }
    assert_eq!(0, index);
    all.reverse();
    (all[..inputs].to_vec(), all[inputs..].to_vec())
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
            let (inputs, outputs) = index_to_io_lossy(i % len, 4, 4, 10);
            assert_eq!(io_to_index(&inputs, &outputs, 10), i % len);
        }
    }
}
