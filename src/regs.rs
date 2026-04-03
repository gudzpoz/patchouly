use patchouly_core::stencils::Location;
use slab::Slab;

pub struct Allocator {
    slots: Slab<Location>,
    registers: Slab<()>,
    stack_slots: Slab<()>,
    stack_size: u16,
}

impl Allocator {
    pub fn new(regs: usize) -> Self {
        let mut reg_pool = Slab::with_capacity(regs);
        for _ in 0..regs {
            reg_pool.insert(());
        }
        for i in (0..regs).rev() {
            reg_pool.remove(i);
        }

        Self {
            registers: reg_pool,
            slots: Default::default(),
            stack_slots: Default::default(),
            stack_size: 0,
        }
    }

    pub fn stack_size(&self) -> u16 {
        self.stack_size
    }

    pub fn location(&self, value: &Value) -> Location {
        self.slots[value.slot as usize]
    }

    pub fn drop(&mut self, value: Value) {
        let location = self.slots.remove(value.slot as usize);
        match location {
            Location::Register(reg) => {
                self.registers.remove(reg as usize);
            }
            Location::Stack(slot) => {
                self.stack_slots.remove(slot as usize);
            }
        }
    }

    pub fn allocate(&mut self, scope: BlockId) -> Option<Value> {
        if self.slots.len() >= u16::MAX as usize {
            return None;
        }
        let location = if self.registers.len() < self.registers.capacity() {
            Location::Register(self.registers.insert(()) as u16)
        } else {
            let slot = self.stack_slots.insert(()).try_into().ok()?;
            self.stack_size = self.stack_size.max(slot + 1);
            Location::Stack(slot)
        };
        Some(Value {
            slot: self.slots.insert(location) as u16,
            scope,
        })
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct BlockId(pub(crate) u16);
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Value {
    slot: u16,
    pub scope: BlockId,
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_lifetime() {
        let mut alloc = Allocator::new(10);
        let reg = alloc.allocate(BlockId(0)).unwrap();
        assert_eq!(reg.slot, 0);
        assert_eq!(alloc.location(&reg), Location::Register(0));
        assert_eq!(1, alloc.slots.len());
        assert_eq!(1, alloc.registers.len());
    }

    #[test]
    fn test_greedy() {
        let mut alloc = Allocator::new(4);
        let regs: Vec<Value> = (0..4)
            .map(|_| alloc.allocate(BlockId(0)).unwrap())
            .collect();
        for (i, reg) in regs.iter().enumerate() {
            assert_eq!(reg.slot as usize, i);
            assert_eq!(alloc.location(reg), Location::Register(i as u16));
        }
        assert_eq!(alloc.slots.len(), 4);
        assert_eq!(alloc.registers.len(), 4);
        let reg = &alloc.allocate(BlockId(0)).unwrap();
        assert_eq!(alloc.location(reg), Location::Stack(0));
    }
}
