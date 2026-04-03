#![feature(rust_preserve_none_cc)]

patchouly::include_stencils!();

fn main() {}

#[cfg(test)]
mod tests {
    use example_commons::{Stack, StackAllocFn};
    use patchouly::patch::PatchBlock;
    use patchouly_core::{
        Stencil, StencilStack,
        stencils::{Location, index_to_io_lossy, io_to_index},
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
    fn test_same_register_input() {
        let duplicate_input_indices = [
            io_to_index(
                &[Location::Register(0), Location::Register(0)],
                &[Location::Stack(0)],
                10,
                false,
            ),
            io_to_index(
                &[Location::Register(0), Location::Register(0)],
                &[Location::Register(2)],
                10,
                false,
            ),
            io_to_index(
                &[Location::Register(8), Location::Register(8)],
                &[Location::Register(1)],
                10,
                false,
            ),
        ];

        for index in duplicate_input_indices {
            assert_ne!(
                stencils::CALC_ADD.stencils[index].into_bits(),
                0,
                "missing duplicate-input stencil at index {index}",
            );
        }
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
        let program = block.finalize().unwrap();
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
        let program = block.finalize().unwrap();
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
    fn test_basic_add_same_input_register_jit() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_ADD,
                &[Location::Register(0), Location::Register(0)],
                &[Location::Register(2)],
                &[],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(2)], &[])
            .unwrap();
        let program = block.finalize().unwrap();
        eprintln!("{:?}", program);
        unsafe {
            let add_same = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut (), usize) -> usize,
            >(program.as_ptr());

            let mut i = 1usize;
            for _ in 0..10000 {
                i = i.wrapping_mul(31);
                let result = add_same(&mut (), i);
                assert_eq!(i.wrapping_mul(2), result);
            }
        }
    }

    #[test]
    fn test_basic_on_stack() {
        test_basic_on_stack_push(false);
        test_basic_on_stack_push(true);
    }

    fn test_basic_on_stack_push(pop: bool) {
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

        if pop {
            block
                .add(
                    &stencils::CALC___MOVE,
                    &[Location::Stack(0)],
                    &[Location::Register(0)],
                    &[],
                )
                .unwrap();
            block
                .add(&stencils::CALC_STACK_POP, &[], &[], &[1])
                .unwrap();
            block
                .ret(&stencils::CALC_RET, &[Location::Register(0)], &[])
                .unwrap();
        } else {
            block
                .ret(&stencils::CALC_RET, &[Location::Stack(0)], &[])
                .unwrap();
        }

        let program = block.finalize().unwrap();
        eprintln!("{:?}", program);
        unsafe {
            let mut stack = Stack(vec![]);
            let mut results = vec![];

            let add2 = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut Stack, usize, usize) -> usize,
            >(program.as_ptr());

            let mut i = 1usize;
            for _ in 0..10000 {
                i = i.wrapping_mul(31);
                let result = add2(&mut stack, i, i);
                let expected = i.wrapping_mul(2);
                results.push(result);
                assert_eq!(expected, result);
                if pop {
                    assert_eq!(0, stack.0.len());
                } else {
                    assert_eq!(expected, stack.get(0));
                }
            }
            if !pop {
                assert_eq!(
                    results,
                    stack.0.iter().map(|v| v.assume_init()).collect::<Vec<_>>()
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_large_hole_value() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_ADD_CONST,
                &[Location::Register(0)],
                &[Location::Register(0)],
                &[1usize << 40],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(0)], &[])
            .unwrap();
        let len = block.measure();
        let program = block.finalize().unwrap();
        assert_eq!(len, Some(program.len()));
        unsafe {
            let add_large = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut (), usize) -> usize,
            >(program.as_ptr());
            assert_eq!((1usize << 40) + 7, add_large(&mut (), 7));
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_multiple_large_hole_values() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .add(
                &stencils::CALC_ADD_CONST,
                &[Location::Register(0)],
                &[Location::Register(0)],
                &[1usize << 40],
            )
            .unwrap();
        block
            .add(
                &stencils::CALC_ADD_CONST,
                &[Location::Register(0)],
                &[Location::Register(0)],
                &[1usize << 41],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(0)], &[])
            .unwrap();
        let len = block.measure();
        let program = block.finalize().unwrap();
        assert_eq!(len, Some(program.len()));

        // constant pool at the end
        let slice = program.as_slice();
        assert_eq!(0, slice.len() % 8);
        assert_eq!(
            1u64 << 40,
            u64::from_le_bytes(slice[slice.len() - 16..slice.len() - 8].try_into().unwrap())
        );
        assert_eq!(
            1u64 << 41,
            u64::from_le_bytes(slice[slice.len() - 8..slice.len()].try_into().unwrap())
        );

        unsafe {
            let add_large = std::mem::transmute::<
                *const u8,
                extern "rust-preserve-none" fn(&mut (), usize) -> usize,
            >(program.as_ptr());
            assert_eq!((1usize << 40) + (1usize << 41) + 7, add_large(&mut (), 7));
        }
    }
}
