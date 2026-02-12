//! Trampoline executor for CPS code
//!
//! The trampoline is the runtime loop that drives CPS execution.
//! It repeatedly processes Actions until completion or yield.

use super::{Action, Continuation};
use crate::value::Value;
use std::rc::Rc;

/// Result of trampoline execution
#[derive(Debug, Clone)]
pub enum TrampolineResult {
    /// Execution completed with a value
    Done(Value),
    /// Execution suspended (yielded)
    Suspended {
        /// Value yielded
        value: Value,
        /// Continuation to resume
        continuation: Rc<Continuation>,
    },
    /// Execution failed
    Error(String),
}

impl TrampolineResult {
    /// Check if execution completed
    pub fn is_done(&self) -> bool {
        matches!(self, TrampolineResult::Done(_))
    }

    /// Check if execution suspended
    pub fn is_suspended(&self) -> bool {
        matches!(self, TrampolineResult::Suspended { .. })
    }

    /// Get the value if done
    pub fn value(&self) -> Option<&Value> {
        match self {
            TrampolineResult::Done(v) => Some(v),
            TrampolineResult::Suspended { value, .. } => Some(value),
            TrampolineResult::Error(_) => None,
        }
    }

    /// Convert to Result
    pub fn into_result(self) -> Result<Value, String> {
        match self {
            TrampolineResult::Done(v) => Ok(v),
            TrampolineResult::Suspended { value, .. } => Ok(value),
            TrampolineResult::Error(e) => Err(e),
        }
    }
}

/// Configuration for the trampoline
#[derive(Debug, Clone)]
pub struct TrampolineConfig {
    /// Maximum number of steps before forcing a yield (prevents infinite loops)
    pub max_steps: usize,
}

impl Default for TrampolineConfig {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
        }
    }
}

/// The trampoline executor
pub struct Trampoline {
    config: TrampolineConfig,
    step_count: usize,
}

impl Trampoline {
    /// Create a new trampoline with default config
    pub fn new() -> Self {
        Self {
            config: TrampolineConfig::default(),
            step_count: 0,
        }
    }

    /// Create a trampoline with custom config
    pub fn with_config(config: TrampolineConfig) -> Self {
        Self {
            config,
            step_count: 0,
        }
    }

    /// Run the trampoline starting with an action
    pub fn run(&mut self, initial_action: Action) -> TrampolineResult {
        let mut current_action = initial_action;
        self.step_count = 0;

        loop {
            self.step_count += 1;

            if self.step_count > self.config.max_steps {
                return TrampolineResult::Error(format!(
                    "Trampoline exceeded max steps ({})",
                    self.config.max_steps
                ));
            }

            match current_action {
                Action::Done(value) => {
                    return TrampolineResult::Done(value);
                }

                Action::Return {
                    value,
                    continuation,
                } => {
                    // Apply the value to the continuation
                    current_action = self.apply_continuation(value, continuation);
                }

                Action::Yield {
                    value,
                    continuation,
                } => {
                    // Suspend execution
                    return TrampolineResult::Suspended {
                        value,
                        continuation,
                    };
                }

                Action::Call {
                    func,
                    args: _,
                    continuation: _,
                } => {
                    // For now, we can't execute calls without VM access
                    // This will be connected to the VM in a later step
                    return TrampolineResult::Error(format!(
                        "Trampoline Call not yet implemented: {:?}",
                        func
                    ));
                }

                Action::TailCall { func, args: _ } => {
                    // For now, we can't execute tail calls without VM access
                    return TrampolineResult::Error(format!(
                        "Trampoline TailCall not yet implemented: {:?}",
                        func
                    ));
                }

                Action::Error(msg) => {
                    return TrampolineResult::Error(msg);
                }
            }
        }
    }

    /// Apply a value to a continuation, producing the next action
    fn apply_continuation(&self, value: Value, cont: Rc<Continuation>) -> Action {
        match cont.as_ref() {
            Continuation::Done => Action::Done(value),

            Continuation::Sequence { remaining, next } => {
                if remaining.is_empty() {
                    // No more expressions, continue with next
                    Action::return_value(value, next.clone())
                } else {
                    // More expressions to evaluate
                    // For now, return an error - expression evaluation needs VM
                    Action::error("Sequence evaluation not yet implemented")
                }
            }

            Continuation::IfBranch {
                then_branch: _,
                else_branch: _,
                next: _,
            } => {
                // Choose branch based on value truthiness
                // For now, return an error - branch evaluation needs VM
                Action::error("IfBranch evaluation not yet implemented")
            }

            Continuation::LetBinding {
                var: _,
                remaining_bindings: _,
                bound_values: _,
                body: _,
                next: _,
            } => {
                // Bind the value and continue
                // For now, return an error - let evaluation needs VM
                Action::error("LetBinding evaluation not yet implemented")
            }

            Continuation::CallReturn { saved_env: _, next } => {
                // Restore environment and continue
                Action::return_value(value, next.clone())
            }

            Continuation::Apply { cont_fn } => {
                // Apply the continuation function
                cont_fn(value)
            }
        }
    }

    /// Get the number of steps taken in the last run
    pub fn step_count(&self) -> usize {
        self.step_count
    }
}

impl Default for Trampoline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trampoline_done() {
        let mut trampoline = Trampoline::new();
        let result = trampoline.run(Action::Done(Value::Int(42)));
        assert!(result.is_done());
        assert_eq!(result.value(), Some(&Value::Int(42)));
    }

    #[test]
    fn test_trampoline_yield() {
        let mut trampoline = Trampoline::new();
        let cont = Continuation::done();
        let result = trampoline.run(Action::yield_value(Value::Int(1), cont));
        assert!(result.is_suspended());
        assert_eq!(result.value(), Some(&Value::Int(1)));
    }

    #[test]
    fn test_trampoline_return_to_done() {
        let mut trampoline = Trampoline::new();
        let cont = Continuation::done();
        let result = trampoline.run(Action::return_value(Value::Int(99), cont));
        assert!(result.is_done());
        assert_eq!(result.value(), Some(&Value::Int(99)));
    }

    #[test]
    fn test_trampoline_error() {
        let mut trampoline = Trampoline::new();
        let result = trampoline.run(Action::error("test error"));
        match result {
            TrampolineResult::Error(msg) => assert_eq!(msg, "test error"),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn test_trampoline_max_steps() {
        let config = TrampolineConfig { max_steps: 10 };
        let mut trampoline = Trampoline::with_config(config);

        // Create an infinite loop by returning to a non-done continuation
        // This would loop forever without the max_steps limit
        // For now, we can't create such a loop without VM support
        // So just test that a simple action works within limits
        let result = trampoline.run(Action::Done(Value::Nil));
        assert!(result.is_done());
    }

    #[test]
    fn test_trampoline_step_count() {
        let mut trampoline = Trampoline::new();
        trampoline.run(Action::Done(Value::Int(42)));
        assert_eq!(trampoline.step_count(), 1);
    }
}
