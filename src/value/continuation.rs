//! Continuation data structures for first-class continuations.
//!
//! A continuation captures the full chain of frames from a yield point
//! up to the coroutine boundary. On resume, the VM replays the chain
//! from innermost to outermost.

use crate::value::Value;
use smallvec::SmallVec;
use std::rc::Rc;

/// An active exception handler in a frame.
///
/// When a yield occurs, the exception handlers active in that frame must be
/// saved so they can be restored on resume. This struct captures the handler
/// state needed to properly route exceptions after resumption.
#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    /// Bytecode offset to jump to when handling an exception
    pub handler_offset: u16,
    /// Optional bytecode offset for finally block (relative, -1 means none)
    pub finally_offset: Option<i16>,
    /// Stack depth when handler was pushed (for unwinding)
    pub stack_depth: usize,
}

/// A single saved execution frame.
///
/// When a coroutine yields, each frame in the call chain is captured
/// with its bytecode, constants, environment, instruction pointer,
/// operand stack state, and exception handler state.
#[derive(Debug, Clone)]
pub struct ContinuationFrame {
    /// The bytecode for this frame
    pub bytecode: Rc<Vec<u8>>,
    /// The constants pool for this frame
    pub constants: Rc<Vec<Value>>,
    /// The closure environment for this frame
    pub env: Rc<Vec<Value>>,
    /// The instruction pointer to resume at (after the Yield/Call instruction)
    pub ip: usize,
    /// The operand stack state for this frame
    pub stack: Vec<Value>,
    /// Exception handlers active in this frame when it was captured
    pub exception_handlers: SmallVec<[ExceptionHandler; 2]>,
    /// Whether this frame was in the middle of handling an exception
    pub handling_exception: bool,
}

/// A captured continuation - the full chain of pending computation.
///
/// When function A calls function B and B yields:
/// - `frames[0]` = B's frame (innermost, the yielder)
/// - `frames[1]` = A's frame (outermost, the caller)
///
/// On resume with value V:
/// 1. Start with innermost frame (B): restore stack, push V, execute from B's saved IP
/// 2. B returns a value -> pop B's frame
/// 3. Continue with next frame (A): push B's return value on A's stack, execute from A's saved IP
/// 4. A returns -> coroutine body is done
#[derive(Debug, Clone)]
pub struct ContinuationData {
    /// The frames in the continuation chain.
    /// Innermost (yielder) first, outermost (caller) last.
    pub frames: Vec<ContinuationFrame>,
}

impl ContinuationData {
    /// Create a new continuation with a single frame (the innermost/yielding frame).
    pub fn new(frame: ContinuationFrame) -> Self {
        ContinuationData {
            frames: vec![frame],
        }
    }

    /// Append a caller's frame to the continuation chain.
    ///
    /// This is called when a yield propagates through a Call instruction.
    /// The caller's frame is added at the end (outermost position).
    /// This is O(1) amortized, unlike the old prepend_frame which was O(n).
    pub fn append_frame(&mut self, frame: ContinuationFrame) {
        self.frames.push(frame);
    }

    /// Check if the continuation has no frames.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Get the number of frames in the continuation.
    pub fn len(&self) -> usize {
        self.frames.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuation_frame_creation() {
        let frame = ContinuationFrame {
            bytecode: Rc::new(vec![1, 2, 3]),
            constants: Rc::new(vec![Value::int(42)]),
            env: Rc::new(vec![]),
            ip: 10,
            stack: vec![Value::int(1), Value::int(2)],
            exception_handlers: SmallVec::new(),
            handling_exception: false,
        };

        assert_eq!(frame.ip, 10);
        assert_eq!(frame.stack.len(), 2);
    }

    #[test]
    fn test_continuation_frame_with_handlers() {
        let handler = ExceptionHandler {
            handler_offset: 100,
            finally_offset: Some(50),
            stack_depth: 3,
        };
        let frame = ContinuationFrame {
            bytecode: Rc::new(vec![1, 2, 3]),
            constants: Rc::new(vec![]),
            env: Rc::new(vec![]),
            ip: 10,
            stack: vec![],
            exception_handlers: smallvec::smallvec![handler],
            handling_exception: true,
        };

        assert_eq!(frame.exception_handlers.len(), 1);
        assert_eq!(frame.exception_handlers[0].handler_offset, 100);
        assert!(frame.handling_exception);
    }

    #[test]
    fn test_continuation_data_append() {
        let inner_frame = ContinuationFrame {
            bytecode: Rc::new(vec![1]),
            constants: Rc::new(vec![]),
            env: Rc::new(vec![]),
            ip: 5,
            stack: vec![],
            exception_handlers: SmallVec::new(),
            handling_exception: false,
        };

        let mut cont = ContinuationData::new(inner_frame);
        assert_eq!(cont.len(), 1);

        let outer_frame = ContinuationFrame {
            bytecode: Rc::new(vec![2]),
            constants: Rc::new(vec![]),
            env: Rc::new(vec![]),
            ip: 10,
            stack: vec![],
            exception_handlers: SmallVec::new(),
            handling_exception: false,
        };

        cont.append_frame(outer_frame);
        assert_eq!(cont.len(), 2);

        // Inner frame should be first (innermost)
        assert_eq!(cont.frames[0].ip, 5);
        // Outer frame should be last (outermost)
        assert_eq!(cont.frames[1].ip, 10);
    }
}
