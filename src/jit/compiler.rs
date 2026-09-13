// audited: 2026-09-13
// docs/impl/jit.md
//! `JitCompiler`: the Cranelift module a compile owns, and the two entry points
//! that drive one `LirFunction` through it.
//!
//! `compile` produces native code and the `JitCode` that keeps it alive;
//! `clif_text` stops at the rendered Cranelift IR, for diagnostics. Both refuse
//! the same two shapes, and the refusals are the whole of what admission asks.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I32, I64};
use cranelift_codegen::ir::{AbiParam, BlockArg, Function, InstBuilder, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

use crate::lir::{Label, LirFunction};
use crate::value::Arity;

use super::code::JitCode;
use super::translate::{load_value_slot, FunctionTranslator};
use super::vtable::{self, RuntimeHelpers};
use super::JitError;

/// What translating one function yields, kept alive by its `JitCode`:
/// closure-template `Value`s referenced by `MakeClosure`, and string-literal
/// template byte buffers the native code's baked pointers point into.
type TranslatedConsts = (
    Vec<std::rc::Rc<crate::value::TemplateProto>>,
    Vec<Box<crate::value::ConstTemplate>>,
);

/// JIT compiler that translates LirFunction to native code
pub struct JitCompiler {
    module: JITModule,
    /// Runtime helper function IDs
    helpers: RuntimeHelpers,
}

mod translate;

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

        // Register all elle_jit_* symbols with the JIT linker
        vtable::register_symbols(&mut builder);

        let mut module = JITModule::new(builder);

        // Declare runtime helper functions
        let helpers = vtable::declare_helpers(&mut module)?;

        Ok(JitCompiler { module, helpers })
    }

    /// Build the standard JIT function signature.
    /// fn(env: *const Value, args: *const Value, nargs: u32, vm: *mut VM,
    ///    self_tag: u64, self_payload: u64) -> JitValue  (two I64s)
    fn make_jit_signature(&self) -> Signature {
        let mut sig = self.module.make_signature();
        sig.call_conv = CallConv::SystemV;
        sig.params.push(AbiParam::new(I64)); // env pointer (*const Value)
        sig.params.push(AbiParam::new(I64)); // args pointer (*const Value)
        sig.params.push(AbiParam::new(I64)); // nargs
        sig.params.push(AbiParam::new(I64)); // vm pointer
        sig.params.push(AbiParam::new(I64)); // self_tag
        sig.params.push(AbiParam::new(I64)); // self_payload
        sig.returns.push(AbiParam::new(I64)); // result tag
        sig.returns.push(AbiParam::new(I64)); // result payload
        sig
    }

    /// Compile a LirFunction to native code
    pub fn compile(
        mut self,
        lir: &LirFunction,
        module_closures: Vec<LirFunction>,
    ) -> Result<JitCode, JitError> {
        // Polymorphic and yielding functions are supported via side-exit.
        // The runtime helper elle_jit_call handles arbitrary callables
        // (closures, arrays, structs), and emit_yield_check_after_call
        // builds a yield-through-call frame if the callee suspends.

        // Variadic functions with struct/named varargs require fiber access
        // for error reporting on invalid keyword arguments. The JIT entry
        // block has no fiber pointer, so these fall back to the interpreter.
        // VarargKind::List variadics are fully supported (pair loop in entry block).
        if matches!(lir.arity, Arity::AtLeast(_))
            && !matches!(lir.vararg_kind, crate::hir::VarargKind::List)
        {
            return Err(JitError::UnsupportedInstruction(
                "variadic function with struct/named varargs".to_string(),
            ));
        }

        // Functions containing MakeClosure fall back to the interpreter. The
        // translator handles MakeClosure (module_closures lookup + bytecode
        // emission), but emitting every module closure's bytecode per compile
        // costs more than the compile saves at a threshold of 1.
        for block in &lir.blocks {
            for si in &block.instructions {
                if matches!(si.instr, crate::lir::LirInstr::MakeClosure { .. }) {
                    return Err(JitError::UnsupportedInstruction("MakeClosure".to_string()));
                }
            }
        }

        // Create function signature
        let sig = self.make_jit_signature();

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
        let (closure_protos, templates) =
            self.translate_function(lir, &mut ctx.func, module_closures)?;

        // Compile the function
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Finalize and get the function pointer
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        let fn_ptr = self.module.get_finalized_function(func_id);
        super::registry::record(fn_ptr as usize, func_name);

        // Convert yield point metadata from LIR to JIT format
        let yield_metas: Vec<super::dispatch::YieldPointMeta> = lir
            .yield_points
            .iter()
            .map(|yp| super::dispatch::YieldPointMeta {
                resume_ip: yp.resume_ip,
                num_spilled: yp.stack_regs.len() as u16,
                num_locals: yp.num_locals,
                num_params: lir.num_params as u16,
            })
            .collect();

        // Convert call site metadata from LIR to JIT format
        let call_site_metas: Vec<super::dispatch::CallSiteMeta> = lir
            .call_sites
            .iter()
            .map(|cs| super::dispatch::CallSiteMeta {
                resume_ip: cs.resume_ip,
                num_spilled: cs.stack_regs.len() as u16,
                num_locals: cs.num_locals,
                num_params: lir.num_params as u16,
            })
            .collect();

        // Wrap in JitCode (module is moved to keep code alive)
        Ok(JitCode::new_with_metadata(
            fn_ptr,
            self.module,
            yield_metas,
            call_site_metas,
            closure_protos,
            templates,
        ))
    }

    /// Build Cranelift IR for a LirFunction and return it as lines of text.
    /// Does NOT compile to native code — this is for diagnostic display only.
    pub fn clif_text(mut self, lir: &LirFunction) -> Result<Vec<String>, JitError> {
        let sig = self.make_jit_signature();

        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        self.translate_function(lir, &mut ctx.func, Vec::new())?;
        // closure_constants from clif_text are discarded — diagnostic only

        let text = format!("{}", ctx.func);
        Ok(text.lines().map(String::from).collect())
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        Self::new().expect("Failed to create JIT compiler")
    }
}

#[cfg(test)]
mod tests;
