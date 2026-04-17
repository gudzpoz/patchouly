#![feature(rust_preserve_none_cc)]

patchouly::include_stencils!();

fn main() {}

#[cfg(test)]
mod tests {
    use example_commons::{BoxedVec, Stack, StackAllocFn};
    use patchouly::{RawFn1, RawFn2, patch::PatchBlock};
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
        let v = Location::Register(0);
        block.emit_vi(&stencils::CALC_ADD_CONST, v, 42, v).unwrap();
        block.ret_v(&stencils::CALC_RET, v).unwrap();
        let add42 = block.finalize_typed::<RawFn1<()>>().unwrap();
        eprintln!("{:?}", add42.program());
        let mut i = 1usize;
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            let result = unsafe { add42.entry() }(&mut (), i);
            assert_eq!(i.wrapping_add(42), result);
        }
    }

    #[test]
    fn test_basic_add_two_jit() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .emit_vv(
                &stencils::CALC_ADD,
                Location::Register(0),
                Location::Register(1),
                Location::Register(8),
            )
            .unwrap();
        block
            .emit_vi(
                &stencils::CALC_ADD_CONST,
                Location::Register(8),
                42,
                Location::Register(4),
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(4)], &[])
            .unwrap();
        let add2_42 = block.finalize_typed::<RawFn2<()>>().unwrap();
        eprintln!("{:?}", add2_42.program());
        let mut i = 1usize;
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            let result = unsafe { add2_42.entry() }(&mut (), i, i);
            assert_eq!(i.wrapping_mul(2).wrapping_add(42), result);
        }
    }

    #[test]
    fn test_basic_add_same_input_register_jit() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        block
            .emit(
                &stencils::CALC_ADD,
                &[Location::Register(0), Location::Register(0)],
                &[Location::Register(2)],
                &[],
            )
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(2)], &[])
            .unwrap();
        let add_same = block.finalize_typed::<RawFn1<()>>().unwrap();
        eprintln!("{:?}", add_same.program());
        let mut i = 1usize;
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            let result = unsafe { add_same.entry() }(&mut (), i);
            assert_eq!(i.wrapping_mul(2), result);
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
            .emit(
                &stencils::CALC_STACK_ALLOC,
                &[],
                &[],
                &[StackAllocFn(Stack::allocate).into(), 1],
            )
            .unwrap();
        let v1 = Location::Register(0);
        let v2 = Location::Register(1);
        let s1 = Location::Stack(0);
        block.emit_vv(&stencils::CALC_ADD, v1, v2, s1).unwrap();

        if pop {
            block.emit_v(&stencils::CALC___MOVE, s1, v1).unwrap();
            block
                .emit(&stencils::CALC_STACK_POP, &[], &[], &[1])
                .unwrap();
            block.ret_v(&stencils::CALC_RET, v1).unwrap();
        } else {
            block.ret_v(&stencils::CALC_RET, s1).unwrap();
        }

        let add2 = block.finalize_typed::<RawFn2<Stack>>().unwrap();
        eprintln!("{:?}", add2.program());
        let mut stack = Stack(vec![]);
        let mut results = vec![];

        let mut i = 1usize;
        for _ in 0..10000 {
            i = i.wrapping_mul(31);
            let result = unsafe { add2.entry() }(&mut stack, i, i);
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
                stack
                    .0
                    .iter()
                    .map(|v| unsafe { v.assume_init() })
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_droppable() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        let v = Location::Register(0);
        block.emit_v(&stencils::CALC_VEC_SUM, v, v).unwrap();
        block.ret_v(&stencils::CALC_RET, v).unwrap();
        let add42 = block.finalize_typed::<RawFn1<()>>().unwrap();
        eprintln!("{:?}", add42.program());
        for i in 0..1000 {
            let mut v = Box::new(vec![0usize; 10_000_000]);
            v[..i].fill(1);
            let result = unsafe { add42.entry() }(&mut (), BoxedVec(v).into());
            assert_eq!(i, result);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_large_hole_value() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        let v = Location::Register(0);
        block
            .emit_vi(&stencils::CALC_ADD_CONST, v, 1 << 40, v)
            .unwrap();
        block.ret_v(&stencils::CALC_RET, v).unwrap();
        let len = block.measure();
        let add_large = block.finalize_typed::<RawFn1<()>>().unwrap();
        assert_eq!(len, Some(add_large.program().len()));
        assert_eq!((1usize << 40) + 7, unsafe { add_large.entry() }(&mut (), 7));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_multiple_large_hole_values() {
        let mut block = PatchBlock::new(&stencils::CALC_STENCIL_LIBRARY);
        let v = Location::Register(0);
        block
            .emit_vi(&stencils::CALC_ADD_CONST, v, 1 << 40, v)
            .unwrap();
        block
            .emit_vi(&stencils::CALC_ADD_CONST, v, 1usize << 41, v)
            .unwrap();
        block
            .ret(&stencils::CALC_RET, &[Location::Register(0)], &[])
            .unwrap();
        let len = block.measure();
        let add_large = block.finalize_typed::<RawFn1<()>>().unwrap();
        assert_eq!(len, Some(add_large.program().len()));

        // constant pool at the end
        let slice = add_large.program().as_slice();
        assert_eq!(0, slice.len() % 8);
        assert_eq!(
            1u64 << 40,
            u64::from_le_bytes(slice[slice.len() - 16..slice.len() - 8].try_into().unwrap())
        );
        assert_eq!(
            1u64 << 41,
            u64::from_le_bytes(slice[slice.len() - 8..slice.len()].try_into().unwrap())
        );

        assert_eq!(
            (1usize << 40) + (1usize << 41) + 7,
            unsafe { add_large.entry() }(&mut (), 7)
        );
    }
}
