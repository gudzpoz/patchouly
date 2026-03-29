include!(concat!(env!("OUT_DIR"), "/calc_stencils.rs"));

fn main() {}

#[cfg(test)]
mod tests {
    use patchouly_core::{
        Stencil,
        stencils::{Variable, index_to_io_lossy},
    };

    use super::stencils;

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test() {
        // jmp to the next stencil
        assert_eq!(b"\xe9\0\0\0\0", stencils::CALC_STENCIL_LIBRARY.empty,);
    }

    type TestStencil = Stencil<0, 0, 0, 0>;

    /// Stencils has strong template parameters to prevent users
    /// from pass wrong number of holes/jumps, at compilation time.
    /// But not so useful for tests.
    macro_rules! get_stencils {
        ($mod:ident::$family:ident) => {
            (
                stringify!($family),
                $mod::$family
                    .stencils
                    .iter()
                    .map(|s| TestStencil::from_bits(s.into_bits()))
                    .collect::<Vec<_>>(),
                (
                    $mod::$family.inputs(),
                    $mod::$family.outputs(),
                    $mod::$family.max_regs(),
                    $mod::$family.holes(),
                    $mod::$family.jumps(),
                ),
            )
        };
    }

    fn has_var_dups(vars: &[Variable]) -> bool {
        let mut bitset = 0usize;
        for var in vars {
            let bit = match var {
                Variable::Stack(_) => continue,
                Variable::Register(i) => i,
            };
            if bitset & (1 << bit) != 0 {
                return true;
            }
            bitset |= 1 << bit;
        }
        false
    }

    #[test]
    fn test_has_code() {
        let families = [
            get_stencils!(stencils::CALC_ADD),
            get_stencils!(stencils::CALC_ADD1),
            get_stencils!(stencils::CALC_ADD_CONST),
            get_stencils!(stencils::CALC_STACK_ALLOC),
        ];

        for (name, family, meta) in families {
            for (i, s) in family.iter().enumerate() {
                if s.into_bits() == 0 {
                    let (inputs, outputs) = index_to_io_lossy(i, meta.0, meta.1, meta.2);
                    assert!(
                        has_var_dups(&inputs) || has_var_dups(&outputs),
                        "duplicate variables: {} {} {:?} {:?}",
                        name,
                        i,
                        inputs,
                        outputs
                    );
                    continue;
                }
                assert!(
                    !s.code(stencils::CALC_STENCIL_LIBRARY.code).is_empty(),
                    "empty code: {} {:?}",
                    name,
                    s,
                );
            }
        }
    }
}
