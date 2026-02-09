// Conditional branching support for Cranelift
//
// Handles if/then/else and other control flow constructs,
// properly managing block parameters and value phi nodes.

use cranelift::prelude::*;

/// Manages conditional branching in CLIF IR
pub struct BranchManager;

impl BranchManager {
    /// Create a new if/then/else block structure
    ///
    /// Returns: (then_block, else_block, join_block)
    pub fn create_if_blocks(builder: &mut FunctionBuilder) -> (Block, Block, Block) {
        let then_block = builder.create_block();
        let else_block = builder.create_block();
        let join_block = builder.create_block();

        (then_block, else_block, join_block)
    }

    /// Emit a conditional branch based on an i64 value
    ///
    /// - If cond != 0, jump to then_block
    /// - Otherwise, jump to else_block
    pub fn emit_if_cond(
        builder: &mut FunctionBuilder,
        cond: cranelift::prelude::Value,
        then_block: Block,
        else_block: Block,
    ) {
        // Create a zero constant for comparison
        let zero = builder.ins().iconst(types::I64, 0);

        // Compare cond with zero
        let cond_is_nonzero = builder.ins().icmp(IntCC::NotEqual, cond, zero);

        // Branch based on condition
        // Note: For now we use brif which takes 4 arguments:
        // cond, block_if_true, args_if_true, block_if_false, args_if_false
        // We'll use empty arg lists for simplicity
        builder
            .ins()
            .brif(cond_is_nonzero, then_block, &[], else_block, &[]);
    }

    /// Set up the join block to receive a value from one of the branches
    pub fn setup_join_block_for_value(join_block: Block, builder: &mut FunctionBuilder) {
        builder.append_block_param(join_block, types::I64);
    }

    /// Jump to join block with a value
    pub fn jump_to_join(
        builder: &mut FunctionBuilder,
        join_block: Block,
        value: cranelift::prelude::Value,
    ) {
        builder.ins().jump(join_block, &[value]);
    }

    /// Get the value passed to join block via phi
    pub fn get_join_value(
        builder: &mut FunctionBuilder,
        join_block: Block,
    ) -> cranelift::prelude::Value {
        builder.block_params(join_block)[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift::codegen::ir;

    #[test]
    fn test_create_if_blocks() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let (then_bl, else_bl, join_bl) = BranchManager::create_if_blocks(&mut builder);

        // Verify blocks are created
        assert!(then_bl.as_u32() > 0);
        assert!(else_bl.as_u32() > 0);
        assert!(join_bl.as_u32() > 0);
    }

    #[test]
    fn test_if_cond_branching() {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut func = ir::Function::new();
        func.signature.params.push(AbiParam::new(types::I64));
        func.signature.returns.push(AbiParam::new(types::I64));
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);

        let entry = builder.create_block();
        builder.append_block_param(entry, types::I64);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let (then_bl, else_bl, _join_bl) = BranchManager::create_if_blocks(&mut builder);

        let cond = builder.block_params(entry)[0];
        BranchManager::emit_if_cond(&mut builder, cond, then_bl, else_bl);

        // Branching created without panicking - test passes
    }
}
