use core::ops::DerefMut;

use alloc::{vec, vec::Vec};

use patchouly_core::{StencilFamily, StencilLibrary, relocation::JumpTarget, stencils::Location};
use smallvec::SmallVec;

use crate::{
    EntrypointSignature, PatchError, Program, TypedProgram,
    patch::{PatchBlock, ProgramBlocks},
    regs::{Allocator, BlockId, Value},
};

struct BlockScope<const MAX_REGS: usize> {
    block: PatchBlock<MAX_REGS>,
    parent: Option<BlockId>,
    variables: SmallVec<[Value; 8]>,
}

pub enum JumpScope {
    Next,
    Child(BlockId),
    Same(BlockId),
    Parent { to: BlockId, parent: BlockId },
}

pub struct PatchFunctionBuilder<const MAX_REGS: usize> {
    library: &'static StencilLibrary<MAX_REGS>,
    allocator: Allocator,
    blocks: Vec<BlockScope<MAX_REGS>>,
    current_scopes: SmallVec<[BlockId; 8]>,
}

impl<const MAX_REGS: usize> PatchFunctionBuilder<MAX_REGS> {
    pub fn new(library: &'static StencilLibrary<MAX_REGS>) -> Self {
        Self {
            library,
            allocator: Allocator::new(MAX_REGS - 1),
            blocks: vec![BlockScope {
                block: PatchBlock::new(library),
                parent: None,
                variables: SmallVec::new(),
            }],
            current_scopes: SmallVec::new(),
        }
    }

    pub fn create_block(&mut self) -> BlockId {
        self.blocks.push(BlockScope {
            block: PatchBlock::new(self.library),
            parent: None,
            variables: SmallVec::new(),
        });
        BlockId(self.blocks.len() as u16 - 1)
    }

    pub fn switch_to_entry(&mut self) -> Result<PatchBlockBuilder<'_, MAX_REGS>, PatchError> {
        self.switch_to_block(BlockId(0))
    }

    pub fn switch_to_block(
        &mut self,
        block: BlockId,
    ) -> Result<PatchBlockBuilder<'_, MAX_REGS>, PatchError> {
        if block.0 as usize >= self.blocks.len() {
            return Err(PatchError::UnresolvedBlockTarget);
        }
        if block.0 != 0 {
            let Some(parent) = &self.blocks[block.0 as usize].parent else {
                return Err(PatchError::BlockOutOfScope);
            };
            let index = self
                .current_scopes
                .iter()
                .position(|v| v == parent)
                .ok_or(PatchError::BlockOutOfScope)?;
            for i in (index + 1..self.current_scopes.len()).rev() {
                for v in self.blocks[self.current_scopes[i].0 as usize]
                    .variables
                    .drain(..)
                {
                    self.allocator.drop(v);
                }
            }
            self.current_scopes.truncate(index + 1);
        }
        self.current_scopes.push(block);
        Ok(PatchBlockBuilder {
            builder: self,
            id: block,
        })
    }

    pub fn finalize_into<F, M>(self, mut allocator: F) -> Result<(M, u16), PatchError>
    where
        F: FnMut(usize) -> Result<(M, usize), PatchError>,
        M: DerefMut<Target = [u8]>,
    {
        let lens = self
            .blocks
            .iter()
            .map(|v| v.block.measure().ok_or(PatchError::NotEnded))
            .collect::<Result<Vec<usize>, PatchError>>()?;
        let (offsets, total) = ProgramBlocks::from_lens(lens);
        let (mut map, base) = allocator(total)?;

        for (i, block) in self.blocks.iter().enumerate() {
            block
                .block
                .finalize_into(&mut map, base, offsets.offsets[i], &offsets)?;
        }

        Ok((map, self.allocator.stack_size()))
    }

    #[cfg(feature = "std")]
    pub fn finalize(self) -> Result<Program, PatchError> {
        let (map, stack_slots) = self.finalize_into(&mut |len| {
            let map = memmap2::MmapMut::map_anon(len)?;
            let base = map.as_ptr() as usize;
            Ok((map, base))
        })?;
        let map = map.make_exec()?;

        Ok(Program {
            mmap: map,
            stack_slots,
        })
    }

    pub fn finalize_into_vec(self, base: Option<usize>) -> Result<(Vec<u8>, u16), PatchError> {
        let (map, stack_slots) = self.finalize_into(&mut |len| {
            let v = vec![0u8; len];
            let base = base.unwrap_or(v.as_ptr() as usize);
            Ok((v, base))
        })?;

        Ok((map, stack_slots))
    }

    pub fn finalize_typed<Sig: EntrypointSignature>(self) -> Result<TypedProgram<Sig>, PatchError> {
        self.finalize().map(Program::into_typed)
    }
}

pub struct PatchBlockBuilder<'a, const MAX_REGS: usize> {
    builder: &'a mut PatchFunctionBuilder<MAX_REGS>,
    id: BlockId,
}
impl<'a, const MAX_REGS: usize> PatchBlockBuilder<'a, MAX_REGS> {
    pub fn builder(&mut self) -> &mut PatchFunctionBuilder<MAX_REGS> {
        self.builder
    }

    pub fn id(&self) -> BlockId {
        self.id
    }

    pub fn new_param(&mut self) -> Result<Value, PatchError> {
        let block = &mut self.builder.blocks[self.id.0 as usize];
        if block.parent.is_some() {
            return Err(PatchError::InvalidParams);
        }
        self.new_variable()
    }

    pub fn new_variable(&mut self) -> Result<Value, PatchError> {
        let value = self
            .builder
            .allocator
            .allocate(self.id)
            .ok_or(PatchError::OutOfVariables)?;
        self.builder.blocks[self.id.0 as usize]
            .variables
            .push(value);
        Ok(value)
    }

    pub fn emit<const IN: usize, const OUT: usize, const HOLES: usize>(
        &'_ mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, 1>,
        inputs: &[Value; IN],
        outputs: &[Value; OUT],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        let block = &mut self.builder.blocks[self.id.0 as usize].block;
        let inputs = &locations(
            &self.builder.allocator,
            &self.builder.current_scopes,
            inputs,
        )?;
        let outputs = &locations(
            &self.builder.allocator,
            &self.builder.current_scopes,
            outputs,
        )?;
        block.emit(stencil, inputs, outputs, holes)
    }

    pub fn end_branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Value; IN],
        outputs: &[Value; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpScope; JUMPS],
        next: BlockId,
    ) -> Result<(), PatchError> {
        let (inputs, outputs, jumps) = self.branch_prep(inputs, outputs, jumps)?;
        let block = &mut self.builder.blocks[self.id.0 as usize].block;
        block.end_branch(stencil, &inputs, &outputs, holes, &jumps)?;
        self.builder.switch_to_block(next)?;
        self.id = next;
        Ok(())
    }

    pub fn branch<const IN: usize, const OUT: usize, const HOLES: usize, const JUMPS: usize>(
        &mut self,
        stencil: &StencilFamily<IN, OUT, MAX_REGS, HOLES, JUMPS>,
        inputs: &[Value; IN],
        outputs: &[Value; OUT],
        holes: &[usize; HOLES],
        jumps: &[JumpScope; JUMPS],
    ) -> Result<(), PatchError> {
        let (inputs, outputs, jumps) = self.branch_prep(inputs, outputs, jumps)?;
        let block = &mut self.builder.blocks[self.id.0 as usize].block;
        block.branch(stencil, &inputs, &outputs, holes, &jumps)
    }

    fn branch_prep<const IN: usize, const OUT: usize, const JUMPS: usize>(
        &mut self,
        inputs: &[Value; IN],
        outputs: &[Value; OUT],
        jumps: &[JumpScope; JUMPS],
    ) -> Result<InputPrep<IN, OUT, JUMPS>, PatchError> {
        let inputs = locations(
            &self.builder.allocator,
            &self.builder.current_scopes,
            inputs,
        )?;
        let outputs = locations(
            &self.builder.allocator,
            &self.builder.current_scopes,
            outputs,
        )?;
        let mut block_out_of_scope = false;
        let jumps = jumps.each_ref().map(|v| match v {
            JumpScope::Child(block_id) => {
                self.builder.blocks[block_id.0 as usize].parent = Some(self.id);
                JumpTarget::Target(block_id.0)
            }
            JumpScope::Same(block_id) => {
                let parent = self.builder.blocks[self.id.0 as usize].parent;
                self.builder.blocks[block_id.0 as usize].parent = parent.or(Some(BlockId(0)));
                JumpTarget::Target(block_id.0)
            }
            JumpScope::Parent { to, parent } => {
                if !self.builder.current_scopes.contains(parent) {
                    block_out_of_scope = true;
                }
                self.builder.blocks[to.0 as usize].parent = Some(*parent);
                JumpTarget::Target(to.0)
            }
            JumpScope::Next => JumpTarget::Next,
        });
        if block_out_of_scope {
            return Err(PatchError::BlockOutOfScope);
        }
        Ok((inputs, outputs, jumps))
    }

    pub fn ret<const IN: usize, const HOLES: usize>(
        self,
        stencil: &StencilFamily<IN, 0, MAX_REGS, HOLES, 0>,
        inputs: &[Value; IN],
        holes: &[usize; HOLES],
    ) -> Result<(), PatchError> {
        let block = &mut self.builder.blocks[self.id.0 as usize].block;
        block.ret(
            stencil,
            &locations(
                &self.builder.allocator,
                &self.builder.current_scopes,
                inputs,
            )?,
            holes,
        )
    }
}

type InputPrep<const IN: usize, const OUT: usize, const JUMPS: usize> =
    ([Location; IN], [Location; OUT], [JumpTarget; JUMPS]);

fn locations<const LEN: usize>(
    alloc: &Allocator,
    scopes: &[BlockId],
    values: &[Value; LEN],
) -> Result<[Location; LEN], PatchError> {
    if values.iter().all(|v| scopes.contains(&v.scope)) {
        Ok(values.each_ref().map(|v| alloc.location(v)))
    } else {
        Err(PatchError::VariableOutOfScope)
    }
}

#[cfg(test)]
mod tests {
    use patchouly_core::{Stencil, StencilFamily, relocation::Relocation};

    use super::*;

    static LIBRARY: StencilLibrary<1> = StencilLibrary {
        code: b"\0",
        empty: b"",
        moves: &StencilFamily {
            relocation_data: &[],
            stencils: &[],
        },
    };

    static JMP: StencilFamily<0, 0, 1, 0, 1> = StencilFamily {
        relocation_data: &[Relocation::new()],
        stencils: &[Stencil {
            code_index: 0,
            code_len: 1,
            relocation_index: 0,
        }],
    };

    static RET: StencilFamily<0, 0, 1, 0, 0> = StencilFamily {
        relocation_data: &[Relocation::new()],
        stencils: &[Stencil {
            code_index: 0,
            code_len: 1,
            relocation_index: 0,
        }],
    };

    #[test]
    fn test_lifetime_rules() {
        let mut builder = PatchFunctionBuilder::new(&LIBRARY);
        let end = builder.create_block();
        let mut block = builder.switch_to_entry().unwrap();
        block.new_variable().unwrap();
        block.new_variable().unwrap();
        block
            .end_branch(&JMP, &[], &[], &[], &[JumpScope::Child(end)], end)
            .unwrap();
        block.ret(&RET, &[], &[]).unwrap();

        let program = builder.finalize().unwrap();
        assert_eq!(program.stack_slots, 2);
    }
}
