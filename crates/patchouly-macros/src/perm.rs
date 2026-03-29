use std::num::NonZero;

use smallvec::SmallVec;

/// Iterate over all argument/return locations
///
/// In the returned vectors, a `0` means the argument/return is put
/// on the stack; otherwise, it is put in a register. Many arguments
/// can be put onto the stack at the same time, but they will not share
/// a register; an argument can share a register with a return value, though.
pub struct RegPermutation {
    inputs: usize,
    current: SmallVec<[u16; 8]>,
    registers: Option<NonZero<u16>>,
}
impl RegPermutation {
    pub fn new(inputs: u16, outputs: u16, registers: NonZero<u16>) -> Self {
        Self {
            inputs: inputs as usize,
            current: SmallVec::from_elem(0, (inputs + outputs) as usize),
            registers: Some(registers),
        }
    }
}
impl Iterator for RegPermutation {
    type Item = SmallVec<[u16; 8]>;

    fn next(&mut self) -> Option<Self::Item> {
        let registers = self.registers?;
        let ret = self.current.clone();
        if self.current.is_empty() {
            self.registers = None;
        }

        'inc: for i in (0..self.current.len()).rev() {
            loop {
                self.current[i] += 1;
                if self.current[i] >= registers.get() {
                    self.current[i] = 0;
                    if i == 0 {
                        self.registers = None;
                    }
                    break;
                }
                let range = if i >= self.inputs {
                    self.inputs..i
                } else {
                    0..i
                };
                if self.current[range].iter().all(|&x| x != self.current[i]) {
                    break 'inc;
                }
            }
        }

        Some(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_arg() {
        let mut iter = RegPermutation::new(0, 0, NonZero::new(1).unwrap());
        assert_eq!(iter.next(), Some(vec![].into()));
        assert_eq!(iter.next(), None);

        let mut iter = RegPermutation::new(0, 0, NonZero::new(10).unwrap());
        assert_eq!(iter.next(), Some(vec![].into()));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_all_on_stack() {
        let mut iter = RegPermutation::new(2, 2, NonZero::new(1).unwrap());
        assert_eq!(iter.next(), Some(vec![0, 0, 0, 0].into()));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_one_arg() {
        let mut iter = RegPermutation::new(0, 1, NonZero::new(4).unwrap());
        assert_eq!(iter.next(), Some(vec![0].into()));
        assert_eq!(iter.next(), Some(vec![1].into()));
        assert_eq!(iter.next(), Some(vec![2].into()));
        assert_eq!(iter.next(), Some(vec![3].into()));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_two_args() {
        let mut iter = RegPermutation::new(1, 1, NonZero::new(2).unwrap());
        assert_eq!(iter.next(), Some(vec![0, 0].into()));
        assert_eq!(iter.next(), Some(vec![0, 1].into()));
        assert_eq!(iter.next(), Some(vec![1, 0].into()));
        assert_eq!(iter.next(), Some(vec![1, 1].into()));
        assert_eq!(iter.next(), None);

        let mut iter = RegPermutation::new(1, 1, NonZero::new(10).unwrap());
        for _ in 0..100 {
            assert!(iter.next().is_some());
        }
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_multiple() {
        let mut iter = RegPermutation::new(2, 1, NonZero::new(3).unwrap());
        assert_eq!(iter.next(), Some(vec![0, 0, 0].into()));
        assert_eq!(iter.next(), Some(vec![0, 0, 1].into()));
        assert_eq!(iter.next(), Some(vec![0, 0, 2].into()));
        assert_eq!(iter.next(), Some(vec![0, 1, 0].into()));
        assert_eq!(iter.next(), Some(vec![0, 1, 1].into()));
        assert_eq!(iter.next(), Some(vec![0, 1, 2].into()));
        assert_eq!(iter.next(), Some(vec![0, 2, 0].into()));
        assert_eq!(iter.next(), Some(vec![0, 2, 1].into()));
        assert_eq!(iter.next(), Some(vec![0, 2, 2].into()));

        assert_eq!(iter.next(), Some(vec![1, 0, 0].into()));
        assert_eq!(iter.next(), Some(vec![1, 0, 1].into()));
        assert_eq!(iter.next(), Some(vec![1, 0, 2].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![1, 1, 0].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![1, 1, 1].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![1, 1, 2].into()));
        assert_eq!(iter.next(), Some(vec![1, 2, 0].into()));
        assert_eq!(iter.next(), Some(vec![1, 2, 1].into()));
        assert_eq!(iter.next(), Some(vec![1, 2, 2].into()));

        assert_eq!(iter.next(), Some(vec![2, 0, 0].into()));
        assert_eq!(iter.next(), Some(vec![2, 0, 1].into()));
        assert_eq!(iter.next(), Some(vec![2, 0, 2].into()));
        assert_eq!(iter.next(), Some(vec![2, 1, 0].into()));
        assert_eq!(iter.next(), Some(vec![2, 1, 1].into()));
        assert_eq!(iter.next(), Some(vec![2, 1, 2].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![2, 2, 0].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![2, 2, 1].into()));
        // SKIP: assert_eq!(iter.next(), Some(vec![2, 2, 2].into()));
        assert_eq!(iter.next(), None);
    }
}
