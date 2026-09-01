//! JIT compiler: LirFunction -> Cranelift IR -> Native code
//!
//! This module translates LIR (Low-level Intermediate Representation) to
//! Cranelift IR, then compiles to native code (x86_64, aarch64).

use std::collections::HashMap;
use std::sync::Arc;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::{I32, I64};
use cranelift_codegen::ir::{AbiParam, BlockArg, Function, InstBuilder, Signature, UserFuncName};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

use crate::lir::{Label, LirFunction};
use crate::value::{Arity, SymbolId};

use super::code::JitCode;
use super::translate::{load_value_slot, FunctionTranslator};
use super::vtable::{self, RuntimeHelpers};
use super::JitError;

/// What translating one function yields, kept alive by its `JitCode`:
/// closure-template `Value`s referenced by `MakeClosure`, and string-literal
/// template byte buffers the native code's baked pointers point into.
type TranslatedConsts = (
    Vec<Box<crate::value::ClosureTemplate>>,
    Vec<Box<crate::value::ConstTemplate>>,
);

/// A member of a compilation group (SCC) for batch JIT compilation.
pub struct BatchMember<'a> {
    /// Symbol ID for this function (used to identify it for direct calls)
    pub sym: SymbolId,
    /// The LIR function to compile
    pub lir: &'a LirFunction,
}

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
        self_sym: Option<SymbolId>,
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

        // Functions containing MakeClosure fall back to the interpreter.
        // The JIT has the infrastructure to handle MakeClosure (module_closures
        // lookup + bytecode emission), but the per-compilation cost of emitting
        // all module closures' bytecodes is too high for --jit=1 threshold.
        // TODO: cache compiled module closures across JIT compilations.
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

        // Build a one-entry scc_peers map for direct self-calls
        let scc_peers = self_sym.map(|sym| {
            let mut map = HashMap::new();
            map.insert(sym, func_id);
            map
        });

        // Create function context
        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        // Translate LIR to Cranelift IR
        let (closure_protos, templates) = self.translate_function(
            lir,
            &mut ctx.func,
            scc_peers.as_ref(),
            self_sym,
            module_closures,
        )?;

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
    pub fn clif_text(
        mut self,
        lir: &LirFunction,
        self_sym: Option<SymbolId>,
    ) -> Result<Vec<String>, JitError> {
        let sig = self.make_jit_signature();

        let func_name = lir.name.as_deref().unwrap_or("jit_func");
        let func_id = self
            .module
            .declare_function(func_name, Linkage::Local, &sig)
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Build a one-entry scc_peers map for direct self-calls
        let scc_peers = self_sym.map(|sym| {
            let mut map = HashMap::new();
            map.insert(sym, func_id);
            map
        });

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        self.translate_function(lir, &mut ctx.func, scc_peers.as_ref(), self_sym, Vec::new())?;
        // closure_constants from clif_text are discarded — diagnostic only

        let text = format!("{}", ctx.func);
        Ok(text.lines().map(String::from).collect())
    }

    /// Compile multiple mutually recursive functions into a single Cranelift module.
    ///
    /// Functions within the group call each other via direct Cranelift `call`
    /// instructions, eliminating the `elle_jit_call` dispatch overhead.
    /// External calls (to functions outside the group) still use `elle_jit_call`.
    pub fn compile_batch(
        mut self,
        members: &[BatchMember],
    ) -> Result<Vec<(SymbolId, JitCode)>, JitError> {
        // Validate all members are non-polymorphic and non-yielding.
        // Yielding functions require per-function YieldPointMeta in JitCode,
        // but compile_batch creates shared JitCode with empty yield_points.
        // If a yielding function were batch-compiled, elle_jit_yield would
        // panic on index-out-of-bounds when looking up yield point metadata.
        for member in members {
            if member.lir.signal.propagates != 0 {
                return Err(JitError::Polymorphic);
            }
            if member.lir.signal.may_suspend() {
                return Err(JitError::Yielding);
            }
            if matches!(member.lir.arity, Arity::AtLeast(_))
                && !matches!(member.lir.vararg_kind, crate::hir::VarargKind::List)
            {
                return Err(JitError::UnsupportedInstruction(
                    "variadic function with struct/named varargs".to_string(),
                ));
            }
        }

        let sig = self.make_jit_signature();

        // Declare all functions upfront so they can reference each other
        let mut func_ids: Vec<(SymbolId, FuncId)> = Vec::with_capacity(members.len());
        let mut scc_peers: HashMap<SymbolId, FuncId> = HashMap::new();

        for (i, member) in members.iter().enumerate() {
            let name = member
                .lir
                .name
                .as_deref()
                .map(|n| format!("scc_{}_{}", i, n))
                .unwrap_or_else(|| format!("scc_{}", i));
            let func_id = self
                .module
                .declare_function(&name, Linkage::Local, &sig)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
            func_ids.push((member.sym, func_id));
            scc_peers.insert(member.sym, func_id);
        }

        // Define each function with the SCC peer map, collecting closure
        // template blueprints so they stay alive as long as the JitCode does.
        let mut all_closure_protos: Vec<(SymbolId, Vec<Box<crate::value::ClosureTemplate>>)> =
            Vec::new();
        let mut all_templates: Vec<(SymbolId, Vec<Box<crate::value::ConstTemplate>>)> = Vec::new();
        for (i, member) in members.iter().enumerate() {
            let (_, func_id) = func_ids[i];
            let mut ctx = self.module.make_context();
            ctx.func.signature = sig.clone();
            ctx.func.name = UserFuncName::user(0, func_id.as_u32());

            let (closure_protos, templates) = self.translate_function(
                member.lir,
                &mut ctx.func,
                Some(&scc_peers),
                Some(member.sym),
                Vec::new(),
            )?;
            all_closure_protos.push((member.sym, closure_protos));
            all_templates.push((member.sym, templates));

            self.module
                .define_function(func_id, &mut ctx)
                .map_err(|e| JitError::CompilationFailed(e.to_string()))?;
        }

        // Finalize all functions at once
        self.module
            .finalize_definitions()
            .map_err(|e| JitError::CompilationFailed(e.to_string()))?;

        // Collect fn_ptrs before moving module into Arc
        let fn_ptrs: Vec<(SymbolId, *const u8)> = func_ids
            .iter()
            .map(|(sym, fid)| (*sym, self.module.get_finalized_function(*fid)))
            .collect();

        // `fn_ptrs` is index-aligned with `members` (both walk `func_ids`'s
        // insertion order), so each entry is recorded under its member's name.
        for (i, (_, ptr)) in fn_ptrs.iter().enumerate() {
            let name = members[i].lir.name.as_deref().unwrap_or("jit_func");
            super::registry::record(*ptr as usize, name);
        }

        // Wrap module in shared Arc so all JitCode entries keep it alive
        let shared_module = Arc::new(super::code::ModuleHolder::new(self.module));

        // Build results — all share the same module, each with its closure +
        // string-literal template constants (both kept alive by the JitCode).
        let mut constants_map: std::collections::HashMap<
            SymbolId,
            Vec<Box<crate::value::ClosureTemplate>>,
        > = all_closure_protos.into_iter().collect();
        let mut templates_map: std::collections::HashMap<
            SymbolId,
            Vec<Box<crate::value::ConstTemplate>>,
        > = all_templates.into_iter().collect();
        let results = fn_ptrs
            .into_iter()
            .map(|(sym, ptr)| {
                let cc = constants_map.remove(&sym).unwrap_or_default();
                let sc = templates_map.remove(&sym).unwrap_or_default();
                (sym, JitCode::new_shared(ptr, shared_module.clone(), cc, sc))
            })
            .collect();

        Ok(results)
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        Self::new().expect("Failed to create JIT compiler")
    }
}

#[cfg(test)]
mod tests;
