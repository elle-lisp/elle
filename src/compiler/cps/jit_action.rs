//! JIT-compatible Action representation
//!
//! Actions need to be representable in a way that JIT code can create
//! and the trampoline can consume.

/// Action tag values for JIT encoding
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionTag {
    Done = 0,
    Yield = 1,
    Call = 2,
    TailCall = 3,
    Return = 4,
    Error = 5,
}

impl ActionTag {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ActionTag::Done),
            1 => Some(ActionTag::Yield),
            2 => Some(ActionTag::Call),
            3 => Some(ActionTag::TailCall),
            4 => Some(ActionTag::Return),
            5 => Some(ActionTag::Error),
            _ => None,
        }
    }
}

/// JIT-compatible action representation
///
/// This struct can be passed between JIT code and Rust.
/// All pointers are encoded as i64 for JIT compatibility.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct JitAction {
    /// Action type tag
    pub tag: u8,
    /// Padding for alignment
    pub _pad: [u8; 7],
    /// Primary value (encoded)
    pub value: i64,
    /// Continuation pointer (for Yield, Call, Return)
    pub continuation: i64,
    /// Function pointer (for Call, TailCall)
    pub func: i64,
    /// Arguments pointer (for Call, TailCall)
    pub args_ptr: i64,
    /// Arguments length (for Call, TailCall)
    pub args_len: i64,
}

impl JitAction {
    /// Create a Done action
    pub fn done(value: i64) -> Self {
        Self {
            tag: ActionTag::Done as u8,
            _pad: [0; 7],
            value,
            continuation: 0,
            func: 0,
            args_ptr: 0,
            args_len: 0,
        }
    }

    /// Create a Yield action
    pub fn yield_value(value: i64, continuation: i64) -> Self {
        Self {
            tag: ActionTag::Yield as u8,
            _pad: [0; 7],
            value,
            continuation,
            func: 0,
            args_ptr: 0,
            args_len: 0,
        }
    }

    /// Create a Call action
    pub fn call(func: i64, args_ptr: i64, args_len: i64, continuation: i64) -> Self {
        Self {
            tag: ActionTag::Call as u8,
            _pad: [0; 7],
            value: 0,
            continuation,
            func,
            args_ptr,
            args_len,
        }
    }

    /// Create a TailCall action
    pub fn tail_call(func: i64, args_ptr: i64, args_len: i64) -> Self {
        Self {
            tag: ActionTag::TailCall as u8,
            _pad: [0; 7],
            value: 0,
            continuation: 0,
            func,
            args_ptr,
            args_len,
        }
    }

    /// Create a Return action
    pub fn return_value(value: i64, continuation: i64) -> Self {
        Self {
            tag: ActionTag::Return as u8,
            _pad: [0; 7],
            value,
            continuation,
            func: 0,
            args_ptr: 0,
            args_len: 0,
        }
    }

    /// Create an Error action
    pub fn error() -> Self {
        Self {
            tag: ActionTag::Error as u8,
            _pad: [0; 7],
            value: 0,
            continuation: 0,
            func: 0,
            args_ptr: 0,
            args_len: 0,
        }
    }

    /// Get the action tag
    pub fn get_tag(&self) -> Option<ActionTag> {
        ActionTag::from_u8(self.tag)
    }

    /// Check if this is a terminal action (Done or Error)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.get_tag(),
            Some(ActionTag::Done) | Some(ActionTag::Error)
        )
    }
}

impl Default for JitAction {
    fn default() -> Self {
        Self::done(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_action_done() {
        let action = JitAction::done(42);
        assert_eq!(action.get_tag(), Some(ActionTag::Done));
        assert_eq!(action.value, 42);
        assert!(action.is_terminal());
    }

    #[test]
    fn test_jit_action_yield() {
        let action = JitAction::yield_value(1, 0x1234);
        assert_eq!(action.get_tag(), Some(ActionTag::Yield));
        assert_eq!(action.value, 1);
        assert_eq!(action.continuation, 0x1234);
        assert!(!action.is_terminal());
    }

    #[test]
    fn test_jit_action_call() {
        let action = JitAction::call(0x1000, 0x2000, 3, 0x3000);
        assert_eq!(action.get_tag(), Some(ActionTag::Call));
        assert_eq!(action.func, 0x1000);
        assert_eq!(action.args_ptr, 0x2000);
        assert_eq!(action.args_len, 3);
        assert_eq!(action.continuation, 0x3000);
    }

    #[test]
    fn test_jit_action_size() {
        // Ensure the struct has predictable size for JIT
        assert_eq!(std::mem::size_of::<JitAction>(), 48);
    }
}
