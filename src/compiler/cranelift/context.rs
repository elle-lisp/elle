// Cranelift JIT context and module management

use super::runtime_helpers::{jit_car, jit_cdr, jit_is_nil};
use cranelift::codegen::Context;
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;

/// Cranelift JIT compilation context
pub struct JITContext {
    pub module: JITModule,
    pub ctx: Context,
    pub builder_ctx: FunctionBuilderContext,
    /// Map of function names to compiled function pointers
    pub functions: HashMap<String, *const u8>,
}

impl JITContext {
    /// Create a new Cranelift JIT context
    pub fn new() -> Result<Self, String> {
        let isa_builder = cranelift_native::builder()
            .map_err(|e| format!("Failed to create ISA builder: {}", e))?;
        let flags =
            cranelift::codegen::settings::Flags::new(cranelift::codegen::settings::builder());
        let isa = isa_builder
            .finish(flags)
            .map_err(|e| format!("Failed to build ISA: {}", e))?;
        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register runtime helper functions
        builder.symbol("jit_is_nil", jit_is_nil as *const u8);
        builder.symbol("jit_car", jit_car as *const u8);
        builder.symbol("jit_cdr", jit_cdr as *const u8);

        let module = JITModule::new(builder);

        Ok(JITContext {
            ctx: module.make_context(),
            builder_ctx: FunctionBuilderContext::new(),
            module,
            functions: HashMap::new(),
        })
    }

    /// Declare a function in the module
    pub fn declare_function(&mut self, name: &str, sig: Signature) -> Result<FuncId, String> {
        self.module
            .declare_function(name, Linkage::Local, &sig)
            .map_err(|e| format!("Failed to declare function '{}': {:?}", name, e))
    }

    /// Define a compiled function in the module
    pub fn define_function(&mut self, func_id: FuncId) -> Result<(), String> {
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| format!("Failed to define function: {:?}", e))
    }

    /// Get a finalized function pointer
    pub fn get_function(&self, func_id: FuncId) -> *const u8 {
        self.module.get_finalized_function(func_id)
    }

    /// Create a new function signature
    pub fn make_signature(&self) -> Signature {
        self.module.make_signature()
    }

    /// Finalize all defined functions
    pub fn finalize(mut self) -> Result<JITModule, String> {
        let _ = self.module.finalize_definitions();
        Ok(self.module)
    }

    /// Clear the context for the next function
    pub fn clear(&mut self) {
        self.ctx.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_context_creation() {
        let result = JITContext::new();
        assert!(result.is_ok(), "Failed to create JIT context");
    }

    #[test]
    fn test_make_signature() {
        let ctx = JITContext::new().expect("Failed to create context");
        let sig = ctx.make_signature();
        assert!(sig.params.is_empty());
    }
}
