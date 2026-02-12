pub mod condition;
use crate::compiler::ast::Expr;
use crate::compiler::effects::Effect;
pub use condition::Condition;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// A wrapper around Value that implements Send by using Arc instead of Rc
/// This is only safe for values that have been checked with is_value_sendable
#[derive(Clone)]
pub struct SendValue(pub Arc<Value>);

impl SendValue {
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(value: Value) -> Self {
        SendValue(Arc::new(value))
    }

    pub fn into_value(self) -> Value {
        // Try to unwrap the Arc, or clone if there are other references
        match Arc::try_unwrap(self.0) {
            Ok(value) => value,
            Err(arc) => (*arc).clone(),
        }
    }
}

// SAFETY: This is only safe because we check that values are sendable before wrapping
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}

/// Symbol ID for interned symbols.
///
/// Symbols are interned for fast comparison (O(1) via ID comparison
/// instead of O(n) string comparison).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// Function arity specification.
///
/// Specifies how many arguments a function accepts.
///
/// # Examples
///
/// ```
/// use elle::value::Arity;
/// assert!(Arity::Exact(2).matches(2));
/// assert!(!Arity::Exact(2).matches(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exact number of arguments required
    Exact(usize),
    /// At least this many arguments
    AtLeast(usize),
    /// Between min and max arguments (inclusive)
    Range(usize, usize),
}

impl Arity {
    pub fn matches(&self, n: usize) -> bool {
        match self {
            Arity::Exact(expected) => n == *expected,
            Arity::AtLeast(min) => n >= *min,
            Arity::Range(min, max) => n >= *min && n <= *max,
        }
    }
}

/// Native function type
pub type NativeFn = fn(&[Value]) -> Result<Value, String>;

/// Cons cell for list construction
#[derive(Debug, Clone, PartialEq)]
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        Cons { first, rest }
    }
}

/// Source AST for JIT compilation of closures
///
/// Stores the original AST of a lambda expression to enable on-demand
/// JIT compilation via the `jit-compile` primitive.
#[derive(Debug, Clone, PartialEq)]
pub struct JitLambda {
    /// Parameter symbols
    pub params: Vec<SymbolId>,
    /// Body expression (the AST)
    pub body: Box<Expr>,
    /// Captured variable info: (symbol_id, depth, index)
    pub captures: Vec<(SymbolId, usize, usize)>,
    /// Effect of the lambda body
    pub effect: Effect,
}

/// Closure with captured environment
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub bytecode: Rc<Vec<u8>>,
    pub arity: Arity,
    pub env: Rc<Vec<Value>>,
    pub num_locals: usize,
    pub num_captures: usize, // Number of captured variables (for env layout)
    pub constants: Rc<Vec<Value>>,
    /// Original AST for JIT compilation (None if not available)
    pub source_ast: Option<Rc<JitLambda>>,
    /// Effect of the closure body
    pub effect: Effect,
}

impl Closure {
    /// Get the effect of this closure
    pub fn effect(&self) -> Effect {
        self.effect
    }
}

/// FFI library handle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibHandle(pub u32);

/// FFI C object handle (opaque pointer to C data)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CHandle {
    /// Raw C pointer
    pub ptr: *const std::ffi::c_void,
    /// Unique ID for this handle
    pub id: u32,
}

impl CHandle {
    /// Create a new C handle
    pub fn new(ptr: *const std::ffi::c_void, id: u32) -> Self {
        CHandle { ptr, id }
    }
}

/// JIT-compiled closure with native code
///
/// Represents a closure that has been compiled to native machine code
/// via the `jit-compile` primitive. The code_ptr points to native code
/// that can be called directly for performance.
#[derive(Clone)]
pub struct JitClosure {
    /// Function pointer to compiled native code
    /// Signature: fn(args: &[Value], env: &[Value]) -> Result<Value, String>
    pub code_ptr: *const u8,
    /// Captured environment (same as regular Closure)
    pub env: Rc<Vec<Value>>,
    /// Arity for argument validation
    pub arity: Arity,
    /// Original closure for fallback/debugging (optional)
    pub source: Option<Rc<Closure>>,
    /// Unique ID for this compiled function (for cache management)
    pub func_id: u64,
    /// Effect of the closure body
    pub effect: Effect,
}

impl fmt::Debug for JitClosure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<jit-closure id={}>", self.func_id)
    }
}

impl PartialEq for JitClosure {
    fn eq(&self, _other: &Self) -> bool {
        // JIT closures are never equal (like regular closures)
        false
    }
}

impl JitClosure {
    /// Get the effect of this JIT closure
    pub fn effect(&self) -> Effect {
        self.effect
    }
}

/// Wrapper for table/struct keys - allows any Value to be a key
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TableKey {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(SymbolId),
    String(String),
}

impl TableKey {
    /// Convert a Value to a TableKey if possible
    pub fn from_value(val: &Value) -> Result<TableKey, String> {
        match val {
            Value::Nil => Ok(TableKey::Nil),
            Value::Bool(b) => Ok(TableKey::Bool(*b)),
            Value::Int(i) => Ok(TableKey::Int(*i)),
            Value::Symbol(id) => Ok(TableKey::Symbol(*id)),
            Value::String(s) => Ok(TableKey::String(s.to_string())),
            _ => Err(format!("Cannot use {} as table key", val.type_name())),
        }
    }
}

/// Exception value for error handling
#[derive(Debug, Clone, PartialEq)]
pub struct Exception {
    /// Error message
    pub message: Rc<str>,
    /// Optional error data
    pub data: Option<Rc<Value>>,
}

impl Exception {
    /// Create a new exception with a message
    pub fn new(message: impl Into<String>) -> Self {
        Exception {
            message: message.into().into(),
            data: None,
        }
    }

    /// Create a new exception with message and data
    pub fn with_data(message: impl Into<String>, data: Value) -> Self {
        Exception {
            message: message.into().into(),
            data: Some(Rc::new(data)),
        }
    }
}

/// Thread handle for concurrent execution
/// Holds the actual `Result<Value>` from the spawned thread
///
/// The result is wrapped in a `SendValue` to allow safe transmission across threads
#[derive(Clone)]
pub struct ThreadHandle {
    /// The result of the spawned thread execution
    /// `Arc<Mutex<>>` allows safe sharing across threads
    /// The `Result` is wrapped in `SendValue` to make it Send
    pub(crate) result: Arc<Mutex<Option<Result<SendValue, String>>>>,
}

/// Coroutine state
#[derive(Debug, Clone)]
pub enum CoroutineState {
    /// Coroutine has not started
    Created,
    /// Coroutine is running
    Running,
    /// Coroutine is suspended (yielded)
    Suspended,
    /// Coroutine has completed
    Done,
    /// Coroutine encountered an error
    Error(String),
}

/// A coroutine value
#[derive(Debug, Clone)]
pub struct Coroutine {
    /// The coroutine's closure
    pub closure: Rc<Closure>,
    /// Current state
    pub state: CoroutineState,
    /// Last yielded value (if suspended)
    pub yielded_value: Option<Value>,
}

impl fmt::Debug for ThreadHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ThreadHandle")
    }
}

impl PartialEq for ThreadHandle {
    fn eq(&self, _other: &Self) -> bool {
        false // Thread handles are never equal
    }
}

/// Core Lisp value type
#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(SymbolId),
    Keyword(SymbolId), // Keywords are self-evaluating, like :name
    String(Rc<str>),
    Cons(Rc<Cons>),
    Vector(Rc<Vec<Value>>),
    Table(Rc<RefCell<BTreeMap<TableKey, Value>>>), // Mutable table
    Struct(Rc<BTreeMap<TableKey, Value>>),         // Immutable struct
    Closure(Rc<Closure>),
    JitClosure(Rc<JitClosure>),
    NativeFn(NativeFn),
    // FFI types
    LibHandle(LibHandle),
    CHandle(CHandle),
    // Exception handling
    Exception(Rc<Exception>),
    // Condition system (new CL-style exceptions)
    Condition(Rc<Condition>),
    // Concurrency
    ThreadHandle(ThreadHandle),
    // Shared mutable cell for captured variables across closures
    Cell(Rc<RefCell<Box<Value>>>),
    // Coroutines (suspendable computations)
    Coroutine(Rc<Coroutine>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::Keyword(a), Value::Keyword(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Cons(a), Value::Cons(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Table(_), Value::Table(_)) => false, // Tables are mutable, never equal
            (Value::Struct(a), Value::Struct(b)) => a == b,
            (Value::Closure(_), Value::Closure(_)) => false, // Closures are never equal
            (Value::JitClosure(_), Value::JitClosure(_)) => false, // JIT closures are never equal
            (Value::NativeFn(_), Value::NativeFn(_)) => false, // Functions are never equal
            (Value::LibHandle(a), Value::LibHandle(b)) => a == b,
            (Value::CHandle(a), Value::CHandle(b)) => a == b,
            (Value::Exception(a), Value::Exception(b)) => a == b,
            (Value::Condition(a), Value::Condition(b)) => a == b,
            (Value::ThreadHandle(a), Value::ThreadHandle(b)) => a == b,
            (Value::Cell(_), Value::Cell(_)) => false, // Cells are mutable, never equal
            (Value::Coroutine(_), Value::Coroutine(_)) => false, // Coroutines are never equal
            _ => false,
        }
    }
}

impl Value {
    #[inline(always)]
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    #[inline(always)]
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn as_int(&self) -> Result<i64, String> {
        match self {
            Value::Int(n) => Ok(*n),
            _ => Err(format!(
                "Type error: expected integer, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_float(&self) -> Result<f64, String> {
        match self {
            Value::Float(f) => Ok(*f),
            Value::Int(n) => Ok(*n as f64),
            _ => Err(format!(
                "Type error: expected number, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_symbol(&self) -> Result<SymbolId, String> {
        match self {
            Value::Symbol(id) => Ok(*id),
            _ => Err(format!(
                "Type error: expected symbol, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_cons(&self) -> Result<&Rc<Cons>, String> {
        match self {
            Value::Cons(cons) => Ok(cons),
            _ => Err(format!(
                "Type error: expected list, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_vector(&self) -> Result<&Rc<Vec<Value>>, String> {
        match self {
            Value::Vector(vec) => Ok(vec),
            _ => Err(format!(
                "Type error: expected vector, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_closure(&self) -> Result<&Rc<Closure>, String> {
        match self {
            Value::Closure(closure) => Ok(closure),
            _ => Err(format!(
                "Type error: expected closure, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_jit_closure(&self) -> Result<&Rc<JitClosure>, String> {
        match self {
            Value::JitClosure(jc) => Ok(jc),
            _ => Err(format!(
                "Type error: expected jit-closure, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_table(&self) -> Result<&Rc<RefCell<BTreeMap<TableKey, Value>>>, String> {
        match self {
            Value::Table(table) => Ok(table),
            _ => Err(format!(
                "Type error: expected table, got {}",
                self.type_name()
            )),
        }
    }

    pub fn as_struct(&self) -> Result<&Rc<BTreeMap<TableKey, Value>>, String> {
        match self {
            Value::Struct(s) => Ok(s),
            _ => Err(format!(
                "Type error: expected struct, got {}",
                self.type_name()
            )),
        }
    }

    /// Check if value is a proper list
    pub fn is_list(&self) -> bool {
        let mut current = self;
        loop {
            match current {
                Value::Nil => return true,
                Value::Cons(cons) => current = &cons.rest,
                _ => return false,
            }
        }
    }

    /// Convert list to Vec
    pub fn list_to_vec(&self) -> Result<Vec<Value>, String> {
        let mut result = Vec::new();
        let mut current = self.clone();
        loop {
            match current {
                Value::Nil => return Ok(result),
                Value::Cons(cons) => {
                    result.push(cons.first.clone());
                    current = cons.rest.clone();
                }
                _ => return Err("Not a proper list".to_string()),
            }
        }
    }

    /// Get a human-readable type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "boolean",
            Value::Int(_) => "integer",
            Value::Float(_) => "float",
            Value::Symbol(_) => "symbol",
            Value::Keyword(_) => "keyword",
            Value::String(_) => "string",
            Value::Cons(_) => "list",
            Value::Vector(_) => "vector",
            Value::Table(_) => "table",
            Value::Struct(_) => "struct",
            Value::Closure(_) => "closure",
            Value::JitClosure(_) => "jit-closure",
            Value::NativeFn(_) => "native-function",
            Value::LibHandle(_) => "library-handle",
            Value::CHandle(_) => "c-handle",
            Value::Exception(_) => "exception",
            Value::Condition(_) => "condition",
            Value::ThreadHandle(_) => "thread-handle",
            Value::Cell(_) => "cell",
            Value::Coroutine(_) => "coroutine",
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Symbol(id) => write!(f, "Symbol({})", id.0),
            Value::Keyword(id) => write!(f, ":{}", id.0),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Cons(cons) => {
                write!(f, "(")?;
                let mut current = Value::Cons(cons.clone());
                while let Value::Cons(ref c) = current {
                    write!(f, "{:?}", c.first)?;
                    match &c.rest {
                        Value::Nil => break,
                        Value::Cons(_) => {
                            write!(f, " ")?;
                            current = c.rest.clone();
                        }
                        other => {
                            write!(f, " . {:?}", other)?;
                            break;
                        }
                    }
                }
                write!(f, ")")
            }
            Value::Vector(vec) => {
                write!(f, "[")?;
                for (i, v) in vec.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                write!(f, "]")
            }
            Value::Table(tbl) => {
                write!(f, "#<table")?;
                if let Ok(borrowed) = tbl.try_borrow() {
                    for (k, v) in borrowed.iter() {
                        write!(f, " {:?}={:?}", k, v)?;
                    }
                } else {
                    write!(f, " <borrowed>")?;
                }
                write!(f, ">")
            }
            Value::Struct(s) => {
                write!(f, "#<struct")?;
                for (k, v) in s.iter() {
                    write!(f, " {:?}={:?}", k, v)?;
                }
                write!(f, ">")
            }
            Value::Closure(_) => write!(f, "<closure>"),
            Value::JitClosure(jc) => write!(f, "{:?}", jc),
            Value::NativeFn(_) => write!(f, "<native-fn>"),
            Value::LibHandle(h) => write!(f, "<library-handle:{}>", h.0),
            Value::CHandle(h) => write!(f, "<c-handle:{}>", h.id),
            Value::Exception(exc) => write!(f, "<exception: {}>", exc.message),
            Value::Condition(cond) => write!(f, "<condition: id={}>", cond.exception_id),
            Value::ThreadHandle(_) => write!(f, "<thread-handle>"),
            Value::Cell(_) => write!(f, "<cell>"),
            Value::Coroutine(_) => write!(f, "<coroutine>"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::String(s) => write!(f, "{}", s),
            Value::Exception(exc) => write!(f, "Exception: {}", exc.message),
            Value::Condition(cond) => write!(f, "Condition(id={})", cond.exception_id),
            _ => write!(f, "{:?}", self),
        }
    }
}

/// Helper function to construct lists
pub fn list(values: Vec<Value>) -> Value {
    values
        .into_iter()
        .rev()
        .fold(Value::Nil, |acc, v| Value::Cons(Rc::new(Cons::new(v, acc))))
}

/// Helper to create cons cell
#[inline]
pub fn cons(first: Value, rest: Value) -> Value {
    Value::Cons(Rc::new(Cons::new(first, rest)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_construction() {
        let l = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert!(l.is_list());
        let vec = l.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_truthy() {
        assert!(Value::Int(0).is_truthy());
        assert!(Value::Bool(true).is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        assert!(!Value::Nil.is_truthy());
    }
}
