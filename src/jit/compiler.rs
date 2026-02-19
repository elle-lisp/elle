//! JIT compiler: LirFunction -> Cranelift IR -> Native code
//!
//! This module translates LIR (Low-level Intermediate Representation) to
//! Cranelift IR, then compiles to native x86_64 code.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::lir::{BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Terminator, UnaryOp};
use crate::value::repr::{PAYLOAD_MASK, TAG_EMPTY_LIST, TAG_FALSE, TAG_INT, TAG_NIL, TAG_TRUE};

use super::code::JitCode;
use super::runtime;
use super::JitError;

/// Helper to create a Variable from a register/slot index
#[inline]
fn var(n: u32) -> Variable {
    Variable::from_u32(n)
}

/// JIT compiler that translates LirFunction to native code
pub struct JitCompiler {
    module: JITModule,
    /// Runtime helper function IDs
    helpers: RuntimeHelpers,
}

/// Pre-declared runtime helper function IDs
struct RuntimeHelpers {
    add: FuncId,
    sub: FuncId,
    mul: FuncId,
    div: FuncId,
    rem: FuncId,
    bit_and: FuncId,
    bit_or: FuncId,
    bit_xor: FuncId,
    shl: FuncId,
    shr: FuncId,
    neg: FuncId,
    not: FuncId,
    bit_not: FuncId,
    eq: FuncId,
    ne: FuncId,
    lt: FuncId,
    le: FuncId,
    gt: FuncId,
    ge: FuncId,
    #[allow(dead_code)]
    cons: FuncId,
    is_nil: FuncId,
    #[allow(dead_code)]
    is_truthy: FuncId,
}

impl JitCompiler {
    /// Create a new JIT compiler
    pub fn new() -> Result<Self, JitError> {
        // Configure Cranelift for the host target
        let mut flag_builder = settings::builder();
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        let isa_builder =
            cranelift_native::builder().map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Create JIT module with runtime symbols
        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register runtime helper symbols
        builder.symbol("elle_jit_add", runtime::elle_jit_add as *const u8);
        builder.symbol("elle_jit_sub", runtime::elle_jit_sub as *const u8);
        builder.symbol("elle_jit_mul", runtime::elle_jit_mul as *const u8);
        builder.symbol("elle_jit_div", runtime::elle_jit_div as *const u8);
        builder.symbol("elle_jit_rem", runtime::elle_jit_rem as *const u8);
        builder.symbol("elle_jit_bit_and", runtime::elle_jit_bit_and as *const u8);
        builder.symbol("elle_jit_bit_or", runtime::elle_jit_bit_or as *const u8);
        builder.symbol("elle_jit_bit_xor", runtime::elle_jit_bit_xor as *const u8);
        builder.symbol("elle_jit_shl", runtime::elle_jit_shl as *const u8);
        builder.symbol("elle_jit_shr", runtime::elle_jit_shr as *const u8);
        builder.symbol("elle_jit_neg", runtime::elle_jit_neg as *const u8);
        builder.symbol("elle_jit_not", runtime::elle_jit_not as *const u8);
        builder.symbol("elle_jit_bit_not", runtime::elle_jit_bit_not as *const u8);
        builder.symbol("elle_jit_eq", runtime::elle_jit_eq as *const u8);
        builder.symbol("elle_jit_ne", runtime::elle_jit_ne as *const u8);
        builder.symbol("elle_jit_lt", runtime::elle_jit_lt as *const u8);
        builder.symbol("elle_jit_le", runtime::elle_jit_le as *const u8);
        builder.symbol("elle_jit_gt", runtime::elle_jit_gt as *const u8);
        builder.symbol("elle_jit_ge", runtime::elle_jit_ge as *const u8);
        builder.symbol("elle_jit_cons", runtime::elle_jit_cons as *const u8);
        builder.symbol("elle_jit_is_nil", runtime::elle_jit_is_nil as *const u8);
        builder.symbol(
            "elle_jit_is_truthy",
            runtime::elle_jit_is_truthy as *const u8,
        );

        let mut module = JITModule::new(builder);

        // Declare runtime helper functions
        let helpers = Self::declare_helpers(&mut module)?;

        Ok(JitCompiler { module, helpers })
    }

    /// Declare runtime helper functions in the module
    fn declare_helpers(module: &mut JITModule) -> Result<RuntimeHelpers, JitError> {
        // Binary function signature: (i64, i64) -> i64
        let mut binary_sig = module.make_signature();
        binary_sig.params.push(AbiParam::new(I64));
        binary_sig.params.push(AbiParam::new(I64));
        binary_sig.returns.push(AbiParam::new(I64));

        // Unary function signature: (i64) -> i64
        let mut unary_sig = module.make_signature();
        unary_sig.params.push(AbiParam::new(I64));
        unary_sig.returns.push(AbiParam::new(I64));

        let declare =
            |module: &mut JITModule, name: &str, sig: &Signature| -> Result<FuncId, JitError> {
                module
                    .declare_function(name, Linkage::Import, sig)
                    .map_err(|e| JitError::CompilationFailed(e.to_string()))
            };

        Ok(RuntimeHelpers {
            add: declare(module, "elle_jit_add", &binary_sig)?,
            sub: declare(module, "elle_jit_sub", &binary_sig)?,
            mul: declare(module, "elle_jit_mul", &binary_sig)?,
            div: declare(module, "elle_jit_div", &binary_sig)?,
            rem: declare(module, "elle_jit_rem", &binary_sig)?,
            bit_and: declare(module, "elle_jit_bit_and", &binary_sig)?,
            bit_or: declare(module, "elle_jit_bit_or", &binary_sig)?,
            bit_xor: declare(module, "elle_jit_bit_xor", &binary_sig)?,
            shl: declare(module, "elle_jit_shl", &binary_sig)?,
            shr: declare(module, "elle_jit_shr", &binary_sig)?,
            neg: declare(module, "elle_jit_neg", &unary_sig)?,
            not: declare(module, "elle_jit_not", &unary_sig)?,
            bit_not: declare(module, "elle_jit_bit_not", &unary_sig)?,
            eq: declare(module, "elle_jit_eq", &binary_sig)?,
            ne: declare(module, "elle_jit_ne", &binary_sig)?,
            lt: declare(module, "elle_jit_lt", &binary_sig)?,
            le: declare(module, "elle_jit_le", &binary_sig)?,
            gt: declare(module, "elle_jit_gt", &binary_sig)?,
            ge: declare(module, "elle_jit_ge", &binary_sig)?,
            cons: declare(module, "elle_jit_cons", &binary_sig)?,
            is_nil: declare(module, "elle_jit_is_nil", &unary_sig)?,
            is_truthy: declare(module, "elle_jit_is_truthy", &unary_sig)?,
        })
    }

    /// Compile a LirFunction to native code
    pub fn compile(mut self, lir: &LirFunction) -> Result<JitCode, JitError> {
        // Check that function is pure
        if !lir.effect.is_pure() {
            return Err(JitError::NotPure);
        }

        // Create function signature
        // fn(env: *const Value, args: *const Value, nargs: u32, globals: *mut ()) -> Value
        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64)); // env pointer
        sig.params.push(AbiParam::new(I64)); // args pointer
        sig.params.push(AbiParam::new(I64)); // nargs (as i64 for simplicity)
        sig.params.push(AbiParam::new(I64)); // globals pointer
        sig.returns.push(AbiParam::new(I64)); // return value

        // Declare the function
        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Create function context
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        // Translate LIR to Cranelift IR
        self.translate_function(lir, &mut ctx.func)?;

        // Compile the function
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Finalize and get the function pointer
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        let fn_ptr = self.module.get_finalized_function(func_id);

        // Wrap in JitCode (module is moved to keep code alive)
        Ok(JitCode::new(fn_ptr, self.module))
    }

    /// Translate LIR function to Cranelift IR
    fn translate_function(
        &mut self,
        lir: &LirFunction,
        func: &mut Function,
    ) -> Result<(), JitError> {
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(func, &mut builder_ctx);

        // Create translator context
        let mut translator = FunctionTranslator::new(&mut self.module, &self.helpers, lir);

        // Declare variables for all registers
        for i in 0..lir.num_regs {
            builder.declare_var(var(i), I64);
        }

        // Create Cranelift blocks for each LIR basic block
        let mut block_map: HashMap<Label, cranelift_codegen::ir::Block> = HashMap::new();
        for bb in &lir.blocks {
            let cl_block = builder.create_block();
            block_map.insert(bb.label, cl_block);
        }

        // Entry block setup
        let entry_block = block_map[&lir.entry];
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Get function parameters
        let env_ptr = builder.block_params(entry_block)[0];
        let args_ptr = builder.block_params(entry_block)[1];
        let _nargs = builder.block_params(entry_block)[2];
        let globals_ptr = builder.block_params(entry_block)[3];

        translator.env_ptr = Some(env_ptr);
        translator.args_ptr = Some(args_ptr);
        translator.globals_ptr = Some(globals_ptr);

        // Note: We don't pre-load arguments into registers here.
        // The LIR uses LoadCapture instructions to access both captures and parameters.
        // LoadCapture with index < num_captures loads from env (captures).
        // LoadCapture with index >= num_captures loads from args (parameters).

        // Translate each basic block
        for bb in &lir.blocks {
            let cl_block = block_map[&bb.label];

            // Skip entry block (already set up)
            if bb.label != lir.entry {
                builder.switch_to_block(cl_block);
                builder.seal_block(cl_block);
            }

            // Translate instructions
            for spanned in &bb.instructions {
                translator.translate_instr(&mut builder, &spanned.instr, &block_map)?;
            }

            // Translate terminator
            translator.translate_terminator(&mut builder, &bb.terminator.terminator, &block_map)?;
        }

        builder.finalize();
        Ok(())
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        Self::new().expect("Failed to create JIT compiler")
    }
}

/// Translator for a single function
struct FunctionTranslator<'a> {
    module: &'a mut JITModule,
    helpers: &'a RuntimeHelpers,
    lir: &'a LirFunction,
    env_ptr: Option<cranelift_codegen::ir::Value>,
    args_ptr: Option<cranelift_codegen::ir::Value>,
    #[allow(dead_code)]
    globals_ptr: Option<cranelift_codegen::ir::Value>,
}

impl<'a> FunctionTranslator<'a> {
    fn new(module: &'a mut JITModule, helpers: &'a RuntimeHelpers, lir: &'a LirFunction) -> Self {
        FunctionTranslator {
            module,
            helpers,
            lir,
            env_ptr: None,
            args_ptr: None,
            globals_ptr: None,
        }
    }

    /// Translate a single LIR instruction
    fn translate_instr(
        &mut self,
        builder: &mut FunctionBuilder,
        instr: &LirInstr,
        _block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match instr {
            LirInstr::Const { dst, value } => {
                let val = self.translate_const(builder, value);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::ValueConst { dst, value } => {
                let bits = value.to_bits();
                let val = builder.ins().iconst(I64, bits as i64);
                builder.def_var(var(dst.0), val);
            }

            LirInstr::Move { dst, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::Dup { dst, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::LoadLocal { dst, slot } => {
                // In LIR, locals are just registers
                let val = builder.use_var(var(*slot as u32));
                builder.def_var(var(dst.0), val);
            }

            LirInstr::StoreLocal { slot, src } => {
                let val = builder.use_var(var(src.0));
                builder.def_var(var(*slot as u32), val);
            }

            LirInstr::LoadCapture { dst, index } => {
                // The LIR uses indices where:
                // - [0, num_captures) are captures (from env)
                // - [num_captures, num_captures + arity) are parameters (from args)
                let num_captures = self.lir.num_captures;
                if *index < num_captures {
                    // Load from closure environment (captures)
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                } else {
                    // Load from arguments array (parameters)
                    let args_ptr = self.args_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCapture without args pointer".to_string())
                    })?;
                    let param_index = *index - num_captures;
                    let offset = (param_index as i32) * 8;
                    let addr = builder.ins().iadd_imm(args_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                }
            }

            LirInstr::LoadCaptureRaw { dst, index } => {
                // Same as LoadCapture for now (Phase 1 doesn't handle cells specially)
                let num_captures = self.lir.num_captures;
                if *index < num_captures {
                    let env_ptr = self.env_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without env pointer".to_string())
                    })?;
                    let offset = (*index as i32) * 8;
                    let addr = builder.ins().iadd_imm(env_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                } else {
                    let args_ptr = self.args_ptr.ok_or_else(|| {
                        JitError::InvalidLir("LoadCaptureRaw without args pointer".to_string())
                    })?;
                    let param_index = *index - num_captures;
                    let offset = (param_index as i32) * 8;
                    let addr = builder.ins().iadd_imm(args_ptr, offset as i64);
                    let val = builder.ins().load(I64, MemFlags::trusted(), addr, 0);
                    builder.def_var(var(dst.0), val);
                }
            }

            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_binary_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::UnaryOp { dst, op, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_unary_helper(builder, *op, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::Compare { dst, op, lhs, rhs } => {
                let lhs_val = builder.use_var(var(lhs.0));
                let rhs_val = builder.use_var(var(rhs.0));
                let result = self.call_compare_helper(builder, *op, lhs_val, rhs_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsNil { dst, src } => {
                let src_val = builder.use_var(var(src.0));
                let result = self.call_helper_unary(builder, self.helpers.is_nil, src_val)?;
                builder.def_var(var(dst.0), result);
            }

            LirInstr::IsPair { dst, src: _ } => {
                // For Phase 1, just return false (not supported)
                let false_val = builder.ins().iconst(I64, TAG_FALSE as i64);
                builder.def_var(var(dst.0), false_val);
            }

            LirInstr::Pop { src: _ } => {
                // No-op in JIT (stack operations are implicit)
            }

            // Unsupported instructions for Phase 1
            LirInstr::Call { .. } => {
                return Err(JitError::UnsupportedInstruction("Call".to_string()));
            }
            LirInstr::TailCall { .. } => {
                return Err(JitError::UnsupportedInstruction("TailCall".to_string()));
            }
            LirInstr::MakeClosure { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeClosure".to_string()));
            }
            LirInstr::Cons { .. } => {
                return Err(JitError::UnsupportedInstruction("Cons".to_string()));
            }
            LirInstr::MakeVector { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeVector".to_string()));
            }
            LirInstr::Car { .. } => {
                return Err(JitError::UnsupportedInstruction("Car".to_string()));
            }
            LirInstr::Cdr { .. } => {
                return Err(JitError::UnsupportedInstruction("Cdr".to_string()));
            }
            LirInstr::MakeCell { .. } => {
                return Err(JitError::UnsupportedInstruction("MakeCell".to_string()));
            }
            LirInstr::LoadCell { .. } => {
                return Err(JitError::UnsupportedInstruction("LoadCell".to_string()));
            }
            LirInstr::StoreCell { .. } => {
                return Err(JitError::UnsupportedInstruction("StoreCell".to_string()));
            }
            LirInstr::StoreCapture { .. } => {
                return Err(JitError::UnsupportedInstruction("StoreCapture".to_string()));
            }
            LirInstr::LoadGlobal { .. } => {
                return Err(JitError::UnsupportedInstruction("LoadGlobal".to_string()));
            }
            LirInstr::StoreGlobal { .. } => {
                return Err(JitError::UnsupportedInstruction("StoreGlobal".to_string()));
            }
            LirInstr::LoadResumeValue { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "LoadResumeValue".to_string(),
                ));
            }
            LirInstr::PushHandler { .. } => {
                return Err(JitError::UnsupportedInstruction("PushHandler".to_string()));
            }
            LirInstr::PopHandler => {
                return Err(JitError::UnsupportedInstruction("PopHandler".to_string()));
            }
            LirInstr::CheckException => {
                return Err(JitError::UnsupportedInstruction(
                    "CheckException".to_string(),
                ));
            }
            LirInstr::MatchException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "MatchException".to_string(),
                ));
            }
            LirInstr::BindException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "BindException".to_string(),
                ));
            }
            LirInstr::LoadException { .. } => {
                return Err(JitError::UnsupportedInstruction(
                    "LoadException".to_string(),
                ));
            }
            LirInstr::ClearException => {
                return Err(JitError::UnsupportedInstruction(
                    "ClearException".to_string(),
                ));
            }
            LirInstr::ReraiseException => {
                return Err(JitError::UnsupportedInstruction(
                    "ReraiseException".to_string(),
                ));
            }
            LirInstr::Throw { .. } => {
                return Err(JitError::UnsupportedInstruction("Throw".to_string()));
            }
            LirInstr::JumpIfFalseInline { .. } => {
                // These are handled by the emitter, not present in final LIR
                return Err(JitError::UnsupportedInstruction(
                    "JumpIfFalseInline".to_string(),
                ));
            }
            LirInstr::JumpInline { .. } => {
                return Err(JitError::UnsupportedInstruction("JumpInline".to_string()));
            }
            LirInstr::LabelMarker { .. } => {
                // No-op marker
            }
        }
        Ok(())
    }

    /// Translate a terminator
    fn translate_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        block_map: &HashMap<Label, cranelift_codegen::ir::Block>,
    ) -> Result<(), JitError> {
        match term {
            Terminator::Return(reg) => {
                let val = builder.use_var(var(reg.0));
                builder.ins().return_(&[val]);
            }

            Terminator::Jump(label) => {
                let target = block_map.get(label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown jump target: {:?}", label))
                })?;
                builder.ins().jump(*target, &[]);
            }

            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let cond_val = builder.use_var(var(cond.0));
                let then_block = block_map.get(then_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown then target: {:?}", then_label))
                })?;
                let else_block = block_map.get(else_label).ok_or_else(|| {
                    JitError::InvalidLir(format!("Unknown else target: {:?}", else_label))
                })?;

                // Check truthiness: value != NIL && value != FALSE
                let nil = builder.ins().iconst(I64, TAG_NIL as i64);
                let false_val = builder.ins().iconst(I64, TAG_FALSE as i64);
                let not_nil = builder.ins().icmp(IntCC::NotEqual, cond_val, nil);
                let not_false = builder.ins().icmp(IntCC::NotEqual, cond_val, false_val);
                let is_truthy = builder.ins().band(not_nil, not_false);

                builder
                    .ins()
                    .brif(is_truthy, *then_block, &[], *else_block, &[]);
            }

            Terminator::Yield { .. } => {
                return Err(JitError::NotPure);
            }

            Terminator::Unreachable => {
                builder
                    .ins()
                    .trap(cranelift_codegen::ir::TrapCode::unwrap_user(0));
            }
        }
        Ok(())
    }

    /// Translate a constant to a Cranelift value
    fn translate_const(
        &self,
        builder: &mut FunctionBuilder,
        value: &LirConst,
    ) -> cranelift_codegen::ir::Value {
        let bits = match value {
            LirConst::Nil => TAG_NIL,
            LirConst::EmptyList => TAG_EMPTY_LIST,
            LirConst::Bool(true) => TAG_TRUE,
            LirConst::Bool(false) => TAG_FALSE,
            LirConst::Int(n) => TAG_INT | ((*n as u64) & PAYLOAD_MASK),
            LirConst::Float(f) => {
                // Use Value::float to handle NaN-boxing correctly
                crate::value::Value::float(*f).to_bits()
            }
            LirConst::String(_) => {
                // Strings require heap allocation - not supported in Phase 1
                // Return NIL as placeholder
                TAG_NIL
            }
            LirConst::Symbol(id) => crate::value::Value::symbol(id.0).to_bits(),
            LirConst::Keyword(id) => crate::value::Value::keyword(id.0).to_bits(),
        };
        builder.ins().iconst(I64, bits as i64)
    }

    /// Call a binary runtime helper
    fn call_binary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: BinOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            BinOp::Add => self.helpers.add,
            BinOp::Sub => self.helpers.sub,
            BinOp::Mul => self.helpers.mul,
            BinOp::Div => self.helpers.div,
            BinOp::Rem => self.helpers.rem,
            BinOp::BitAnd => self.helpers.bit_and,
            BinOp::BitOr => self.helpers.bit_or,
            BinOp::BitXor => self.helpers.bit_xor,
            BinOp::Shl => self.helpers.shl,
            BinOp::Shr => self.helpers.shr,
        };
        self.call_helper_binary(builder, func_id, lhs, rhs)
    }

    /// Call a unary runtime helper
    fn call_unary_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: UnaryOp,
        src: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            UnaryOp::Neg => self.helpers.neg,
            UnaryOp::Not => self.helpers.not,
            UnaryOp::BitNot => self.helpers.bit_not,
        };
        self.call_helper_unary(builder, func_id, src)
    }

    /// Call a comparison runtime helper
    fn call_compare_helper(
        &mut self,
        builder: &mut FunctionBuilder,
        op: CmpOp,
        lhs: cranelift_codegen::ir::Value,
        rhs: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_id = match op {
            CmpOp::Eq => self.helpers.eq,
            CmpOp::Ne => self.helpers.ne,
            CmpOp::Lt => self.helpers.lt,
            CmpOp::Le => self.helpers.le,
            CmpOp::Gt => self.helpers.gt,
            CmpOp::Ge => self.helpers.ge,
        };
        self.call_helper_binary(builder, func_id, lhs, rhs)
    }

    /// Call a binary helper function
    fn call_helper_binary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
        b: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a, b]);
        Ok(builder.inst_results(call)[0])
    }

    /// Call a unary helper function
    fn call_helper_unary(
        &mut self,
        builder: &mut FunctionBuilder,
        func_id: FuncId,
        a: cranelift_codegen::ir::Value,
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[a]);
        Ok(builder.inst_results(call)[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::lir::{BasicBlock, Reg, SpannedInstr, SpannedTerminator};
    use crate::syntax::Span;

    fn make_simple_lir() -> LirFunction {
        // Create a simple function that returns its first argument
        // fn(x) -> x
        // The LIR uses LoadCapture to access parameters.
        // With num_captures=0, LoadCapture index 0 loads from args[0].
        let mut func = LirFunction::new(1);
        func.num_regs = 1;
        func.num_captures = 0;
        func.effect = Effect::Pure;

        let mut entry = BasicBlock::new(Label(0));
        // Load argument 0 into register 0
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    fn make_add_lir() -> LirFunction {
        // Create a function that adds two arguments
        // fn(x, y) -> x + y
        // With num_captures=0, LoadCapture index 0 and 1 load from args[0] and args[1].
        let mut func = LirFunction::new(2);
        func.num_regs = 3;
        func.num_captures = 0;
        func.effect = Effect::Pure;

        let mut entry = BasicBlock::new(Label(0));
        // Load arguments into registers
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::LoadCapture {
                dst: Reg(1),
                index: 1,
            },
            Span::synthetic(),
        ));
        entry.instructions.push(SpannedInstr::new(
            LirInstr::BinOp {
                dst: Reg(2),
                op: BinOp::Add,
                lhs: Reg(0),
                rhs: Reg(1),
            },
            Span::synthetic(),
        ));
        entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), Span::synthetic());

        func.blocks.push(entry);
        func.entry = Label(0);
        func
    }

    #[test]
    fn test_compile_identity() {
        let lir = make_simple_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let code = compiler.compile(&lir).expect("Failed to compile");

        // Call the compiled function
        let args = [crate::value::Value::int(42).to_bits()];
        let result = unsafe { code.call(std::ptr::null(), args.as_ptr(), 1, std::ptr::null_mut()) };
        let value = unsafe { crate::value::Value::from_bits(result) };
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_compile_add() {
        let lir = make_add_lir();
        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let code = compiler.compile(&lir).expect("Failed to compile");

        // Call the compiled function
        let args = [
            crate::value::Value::int(10).to_bits(),
            crate::value::Value::int(32).to_bits(),
        ];
        let result = unsafe { code.call(std::ptr::null(), args.as_ptr(), 2, std::ptr::null_mut()) };
        let value = unsafe { crate::value::Value::from_bits(result) };
        assert_eq!(value.as_int(), Some(42));
    }

    #[test]
    fn test_reject_non_pure() {
        let mut lir = make_simple_lir();
        lir.effect = Effect::Yields;

        let compiler = JitCompiler::new().expect("Failed to create compiler");
        let result = compiler.compile(&lir);
        assert!(matches!(result, Err(JitError::NotPure)));
    }
}
