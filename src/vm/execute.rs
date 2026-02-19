//! Bytecode execution entry points and helpers.
//!
//! This module contains the public execution methods and the tail call loop.

use crate::value::{ExceptionHandler, Value};
use std::rc::Rc;

use super::core::{VmResult, VM};

impl VM {
    /// Execute bytecode starting from a specific instruction pointer.
    /// This is the main entry point for bytecode execution.
    /// Handler isolation happens at the Call instruction level, not here.
    fn execute_bytecode_inner_with_ip(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        // Each bytecode frame has its own exception handler scope.
        let saved_handlers = std::mem::take(&mut self.exception_handlers);
        let saved_handling = self.handling_exception;
        self.handling_exception = false;

        let result = self.execute_bytecode_inner_impl(bytecode, constants, closure_env, start_ip);

        self.exception_handlers = saved_handlers;
        self.handling_exception = saved_handling;

        result
    }

    /// Wrapper that calls execute_bytecode_inner_impl with start_ip = 0
    pub(super) fn execute_bytecode_inner(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<VmResult, String> {
        self.execute_bytecode_inner_with_ip(bytecode, constants, closure_env, 0)
    }

    /// Execute bytecode starting from a specific instruction pointer.
    /// Used for resuming coroutines from where they yielded.
    pub fn execute_bytecode_from_ip(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
    ) -> Result<VmResult, String> {
        self.execute_bytecode_inner_with_ip(bytecode, constants, closure_env, start_ip)
    }

    /// Execute bytecode starting from a specific IP with pre-set exception handler state.
    ///
    /// This is used when resuming a continuation frame that had active exception handlers
    /// when it was captured. Unlike `execute_bytecode_from_ip`, this method:
    /// 1. Sets the exception handlers to the provided state before execution
    /// 2. Restores the outer handlers after execution
    ///
    /// This ensures that `handler-case` blocks active at yield time remain active
    /// after resume.
    pub fn execute_bytecode_from_ip_with_state(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
        start_ip: usize,
        handlers: Vec<ExceptionHandler>,
        handling: bool,
    ) -> Result<VmResult, String> {
        // Save outer state
        let saved_handlers = std::mem::replace(&mut self.exception_handlers, handlers);
        let saved_handling = std::mem::replace(&mut self.handling_exception, handling);

        // Execute with tail call loop
        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();
        let mut current_ip = start_ip;

        let result = loop {
            let result = self.execute_bytecode_inner_impl(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
                current_ip,
            )?;

            // Check for pending tail call
            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
                current_ip = 0; // Tail calls start from the beginning
            } else {
                break result;
            }
        };

        // Restore outer state
        self.exception_handlers = saved_handlers;
        self.handling_exception = saved_handling;

        Ok(result)
    }

    /// Execute bytecode returning VmResult (for coroutine execution).
    pub fn execute_bytecode_coroutine(
        &mut self,
        bytecode: &[u8],
        constants: &[Value],
        closure_env: Option<&Rc<Vec<Value>>>,
    ) -> Result<VmResult, String> {
        // Save the caller's stack
        let saved_stack = std::mem::take(&mut self.stack);

        let mut current_bytecode = bytecode.to_vec();
        let mut current_constants = constants.to_vec();
        let mut current_env = closure_env.cloned();

        let result = loop {
            let result = self.execute_bytecode_inner(
                &current_bytecode,
                &current_constants,
                current_env.as_ref(),
            )?;

            if let Some((tail_bytecode, tail_constants, tail_env)) = self.pending_tail_call.take() {
                current_bytecode = tail_bytecode;
                current_constants = tail_constants;
                current_env = Some(tail_env);
            } else {
                break result;
            }
        };

        // Restore the caller's stack
        self.stack = saved_stack;

        Ok(result)
    }
}
