#![feature(rust_preserve_none_cc)]

include!(concat!(env!("OUT_DIR"), "/calc_stencils.rs"));

fn main() {}

#[cfg(test)]
mod tests {
    use example_calc_stencils::{Stack, StackAllocFn};
    use patchouly::PatchBlock;
    use patchouly_core::{
        Stencil, StencilStack,
        stencils::{Location, index_to_io_lossy, stencils_len},
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

    #[test]
    fn test_has_code() {
        let families = [
            get_stencils!(stencils::CALC___MOVE),
            get_stencils!(stencils::CALC_ADD),
            get_stencils!(stencils::CALC_ADD1),
            get_stencils!(stencils::CALC_ADD_CONST),
            get_stencils!(stencils::CALC_STACK_ALLOC),
            get_stencils!(stencils::CALC_STACK_POP),
            get_stencils!(stencils::CALC_IF_EQ),
        ];

        for (name, family, meta) in &families {
            let mut empty_count = 0;
            for (i, s) in family.iter().enumerate() {
                if s.into_bits() == 0 {
                    let mut inputs = vec![Location::Stack(0); meta.0];
                    let mut outputs = vec![Location::Stack(0); meta.1];
                    index_to_io_lossy(i, meta.2, &mut inputs, &mut outputs);
                    assert!(
                        has_var_dups(&inputs) || has_var_dups(&outputs),
                        "duplicate variables: {} {} {:?} {:?}",
                        name,
                        i,
                        inputs,
                        outputs
                    );
                    empty_count += 1;
                    continue;
                }
                assert!(
                    !s.code(stencils::CALC_STENCIL_LIBRARY.code).is_empty(),
                    "empty code: {} {:?}",
                    name,
                    s,
                );
            }
            assert!(empty_count * 2 < family.len());
        }
    }

    #[test]
    fn test_empty_count() {
        assert_eq!(1000000, stencils_len(6, 0, 10));
        let mut empty_count = 0;
        for i in 0..1000000 {
            let mut inputs = vec![Location::Stack(0); 6];
            index_to_io_lossy(i, 10, &mut inputs, &mut []);
            if has_var_dups(&inputs) {
                empty_count += 1;
            }
        }
        // TODO: make the index more compact?
        assert_eq!(empty_count, 792225);
    }

    fn has_var_dups(vars: &[Location]) -> bool {
        let mut bitset = 0usize;
        for var in vars {
            let bit = match var {
                Location::Stack(_) => continue,
                Location::Register(i) => i,
            };
            if bitset & (1 << bit) != 0 {
                return true;
            }
            bitset |= 1 << bit;
        }
        false
    }

    #[test]
    fn test_basic_add42_jit() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_ADD_CONST,
                &[Location::Register(0)],
                &[Location::Register(0)],
                &[42],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(0)], &[])
            .unwrap();
        let program = block.finalize(&Default::default()).unwrap();
        eprintln!("{:?}", program);
        unsafe {
            let add42 = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut (), usize) -> usize,
            >(program.as_ptr());

            let mut i = 1usize;
            for _ in 0..10000 {
                i = i.wrapping_mul(31);
                let result = add42(&mut (), i);
                assert_eq!(i.wrapping_add(42), result);
            }
        }
    }

    #[test]
    fn test_basic_add_two_jit() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_ADD,
                &[Location::Register(0), Location::Register(1)],
                &[Location::Register(8)],
                &[],
            )
            .unwrap();
        block
            .add(
                &stencils::CALC_ADD_CONST,
                &[Location::Register(8)],
                &[Location::Register(4)],
                &[42],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(4)], &[])
            .unwrap();
        let program = block.finalize(&Default::default()).unwrap();
        eprintln!("{:?}", program);
        unsafe {
            let add2_42 = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut (), usize, usize) -> usize,
            >(program.as_ptr());

            let mut i = 1usize;
            for _ in 0..10000 {
                i = i.wrapping_mul(31);
                let result = add2_42(&mut (), i, i);
                assert_eq!(i.wrapping_mul(2).wrapping_add(42), result);
            }
        }
    }

    #[test]
    fn test_basic_on_stack() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_STACK_ALLOC,
                &[],
                &[],
                &[StackAllocFn(Stack::allocate).into(), 1],
            )
            .unwrap();
        block
            .add(
                &stencils::CALC_ADD,
                &[Location::Register(0), Location::Register(1)],
                &[Location::Stack(0)],
                &[],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Stack(0)], &[])
            .unwrap();
        let program = block.finalize(&Default::default()).unwrap();
        eprintln!("{:?}", program);
        unsafe {
            let mut stack = Stack(vec![]);
            // TODO: not reserving leads to failed calls to Stack::allocate
            stack.0.reserve(10000);
            let add2 = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut Stack, usize, usize) -> usize,
            >(program.as_ptr());

            let mut i = 1usize;
            for _ in 0..10000 {
                i = i.wrapping_mul(31);
                let result = add2(&mut stack, i, i);
                let expected = i.wrapping_mul(2);
                assert_eq!(expected, result);
                assert_eq!(expected, stack.get(0));
            }
        }
    }
}
