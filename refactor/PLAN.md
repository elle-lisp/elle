# Elle 2.0 Implementation Plan

> **Status as of February 2025**: Phases 1 (Value), 3 (Syntax), 4 (HIR), 5 (LIR),
> and 6 (Scope) are complete — but diverged from plan in several ways. Phase 2
> (Error system) has not started. Phase 7 is partial. Phase 8 is in progress.
> See status notes on each phase below.

This document provides detailed implementation guidance for the Elle 2.0 refactoring.
It synthesizes recommendations from five architectural reviews and provides concrete
file-level changes, code examples, and testing strategies.

**Design Priorities** (in order):
1. Language elegance
2. Performance  
3. C integration
4. Predictable memory

---

## Table of Contents

1. [Phase 1: Value Representation](#phase-1-value-representation)
2. [Phase 2: Error System](#phase-2-error-system)
3. [Phase 3: Syntax Type](#phase-3-syntax-type)
4. [Phase 4: HIR (High-Level IR)](#phase-4-hir)
5. [Phase 5: LIR (Low-Level IR)](#phase-5-lir)
6. [Phase 6: Scope System](#phase-6-scope-system)
7. [Phase 7: Semantics Completion](#phase-7-semantics-completion)
8. [Phase 8: Cleanup](#phase-8-cleanup)
9. [Appendices](#appendices)

---

## Phase 1: Value Representation — ✅ COMPLETE (diverged)

**Duration**: 2-3 weeks  
**Risk**: Critical  
**Dependencies**: None (start here)

### 1.1 Current State

The current `Value` enum in `src/value.rs` has 22 variants and is approximately 24 bytes:

```rust
pub enum Value {
    Nil, Bool(bool), Int(i64), Float(f64),
    Symbol(SymbolId), Keyword(SymbolId), String(Rc<str>),
    Cons(Rc<Cons>), Vector(Rc<Vec<Value>>),
    Table(Rc<RefCell<BTreeMap<...>>>), Struct(Rc<BTreeMap<...>>),
    Closure(Rc<Closure>), JitClosure(Rc<JitClosure>),
    NativeFn(NativeFn), VmAwareFn(VmAwareFn),
    LibHandle(LibHandle), CHandle(CHandle),
    Exception(Rc<Exception>), Condition(Rc<Condition>),
    ThreadHandle(ThreadHandle),
    Cell(Rc<RefCell<Box<Value>>>), LocalCell(Rc<RefCell<Box<Value>>>),
    Coroutine(Rc<RefCell<Coroutine>>),
}
```

**Problems**:
- 24 bytes per value (enum discriminant + largest variant)
- Separate `Rc` allocation for every heap value
- `Cell` vs `LocalCell` distinction is user-facing complexity
- `Closure` vs `JitClosure` duplicated representations
- `Exception` variant (legacy) coexists with `Condition` (new)

### 1.2 Target State

NaN-boxed 8-byte representation:

```rust
/// 8-byte NaN-boxed value
/// 
/// Encoding:
/// - Floats: Any valid f64 that is NOT a NaN
/// - Tagged values: NaN with payload in lower 48 bits
///   - Nil:     0x7FF8_0000_0000_0001
///   - True:    0x7FF8_0000_0000_0002
///   - False:   0x7FF8_0000_0000_0003
///   - Int:     0x7FFC_XXXX_XXXX_XXXX (48-bit signed integer)
///   - Symbol:  0x7FFD_0000_XXXX_XXXX (32-bit symbol ID)
///   - Pointer: 0x7FFE_XXXX_XXXX_XXXX (48-bit pointer to HeapObject)
#[derive(Clone, Copy)]
pub struct Value(u64);

/// All heap-allocated value types
pub enum HeapObject {
    String(Box<str>),
    Cons(Cons),
    Vector(Vec<Value>),
    Table(RefCell<BTreeMap<TableKey, Value>>),
    Struct(BTreeMap<TableKey, Value>),
    Closure(Closure),
    Condition(Condition),
    Coroutine(RefCell<Coroutine>),
    Cell(RefCell<Value>),
    // FFI types
    LibHandle(u32),
    CHandle(*const c_void, u32),
    // Native functions stored as indices into a registry
    NativeFn(u32),
}
```

### 1.3 Implementation Steps

#### Step 1.3.1: Create `src/value/` module structure

```
src/value/
├── mod.rs           # Public API, Value struct
├── repr.rs          # NaN-boxing implementation
├── heap.rs          # HeapObject enum
├── accessors.rs     # as_int(), as_closure(), etc.
├── display.rs       # Debug, Display implementations
├── arena.rs         # Arena allocator (optional, can defer)
└── condition.rs     # Existing, moved here
```

#### Step 1.3.2: Implement NaN-boxing in `src/value/repr.rs`

```rust
const QNAN: u64 = 0x7FFC_0000_0000_0000;
const TAG_NIL: u64 = 0x7FF8_0000_0000_0001;
const TAG_TRUE: u64 = 0x7FF8_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FF8_0000_0000_0003;
const TAG_INT: u64 = 0x7FFC_0000_0000_0000;  // + 48-bit signed int
const TAG_SYMBOL: u64 = 0x7FFD_0000_0000_0000;  // + 32-bit symbol ID
const TAG_POINTER: u64 = 0x7FFE_0000_0000_0000;  // + 48-bit pointer

impl Value {
    pub const NIL: Value = Value(TAG_NIL);
    pub const TRUE: Value = Value(TAG_TRUE);
    pub const FALSE: Value = Value(TAG_FALSE);
    
    #[inline]
    pub fn int(n: i64) -> Value {
        // Truncate to 48 bits (range: -140_737_488_355_328 to 140_737_488_355_327)
        debug_assert!(n >= -(1i64 << 47) && n < (1i64 << 47), "Integer overflow");
        Value(TAG_INT | ((n as u64) & 0x0000_FFFF_FFFF_FFFF))
    }
    
    #[inline]
    pub fn float(f: f64) -> Value {
        let bits = f.to_bits();
        // If it's a NaN, box it as a heap float
        if (bits & QNAN) == QNAN {
            Value::heap(HeapObject::Float(f))
        } else {
            Value(bits)
        }
    }
    
    #[inline]
    pub fn symbol(id: SymbolId) -> Value {
        Value(TAG_SYMBOL | (id.0 as u64))
    }
    
    #[inline]
    pub fn heap(obj: HeapObject) -> Value {
        let ptr = Box::into_raw(Box::new(obj));
        Value(TAG_POINTER | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
    }
    
    #[inline]
    pub fn is_nil(&self) -> bool { self.0 == TAG_NIL }
    
    #[inline]
    pub fn is_int(&self) -> bool { (self.0 & TAG_INT) == TAG_INT && (self.0 & 0x0003) == 0 }
    
    #[inline]
    pub fn is_heap(&self) -> bool { (self.0 & TAG_POINTER) == TAG_POINTER }
    
    #[inline]
    pub fn as_int(&self) -> Option<i64> {
        if self.is_int() {
            // Sign-extend from 48 bits
            let raw = (self.0 & 0x0000_FFFF_FFFF_FFFF) as i64;
            let sign_bit = raw & (1 << 47);
            if sign_bit != 0 {
                Some(raw | !0x0000_FFFF_FFFF_FFFF_u64 as i64)
            } else {
                Some(raw)
            }
        } else {
            None
        }
    }
    
    #[inline]
    pub fn as_heap(&self) -> Option<&HeapObject> {
        if self.is_heap() {
            let ptr = (self.0 & 0x0000_FFFF_FFFF_FFFF) as *const HeapObject;
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }
}
```

#### Step 1.3.3: Merge Cell types

Before:
```rust
Cell(Rc<RefCell<Box<Value>>>),      // User-created via `box`
LocalCell(Rc<RefCell<Box<Value>>>), // Compiler-created for captures
```

After:
```rust
// In HeapObject
Cell(RefCell<Value>),  // Single cell type

// Cell metadata moves to Closure
pub struct Closure {
    // ...
    /// Bitmap: bit N = 1 means env[N] is a cell that should be auto-unwrapped
    pub cell_mask: u64,
}
```

#### Step 1.3.4: Merge Closure types

Before:
```rust
Closure(Rc<Closure>),
JitClosure(Rc<JitClosure>),
```

After:
```rust
pub struct Closure {
    pub bytecode: Option<Rc<Vec<u8>>>,  // None if JIT-only
    pub jit_code: Option<*const u8>,     // None if not JIT'd
    pub arity: Arity,
    pub env: Vec<Value>,                 // Captured values
    pub cell_mask: u64,                  // Which env slots are cells
    pub constants: Rc<Vec<Value>>,
    pub effect: Effect,
    pub source_ast: Option<Rc<JitLambda>>,
}
```

#### Step 1.3.5: Remove Exception variant

Before:
```rust
Exception(Rc<Exception>),
Condition(Rc<Condition>),
```

After:
```rust
// Only Condition remains (in HeapObject)
Condition(Condition),
```

Migrate all `Exception` usage to `Condition`.

### 1.4 Files to Modify

| File | Changes |
|------|---------|
| `src/value.rs` | Delete (replace with `src/value/mod.rs`) |
| `src/value/mod.rs` | New: public API |
| `src/value/repr.rs` | New: NaN-boxing |
| `src/value/heap.rs` | New: HeapObject |
| `src/value/accessors.rs` | New: type accessors |
| `src/value/display.rs` | New: Debug/Display |
| `src/vm/mod.rs` | Update all `Value::` patterns |
| `src/vm/*.rs` | Update all instruction handlers |
| `src/primitives/*.rs` | Update all ~100 primitive functions |
| `src/compiler/compile/mod.rs` | Update literal compilation |
| `src/compiler/cps/*.rs` | Update CPS value handling |

### 1.5 Testing Strategy

1. **Before starting**: Run `cargo test` and record baseline
2. **Unit tests for NaN-boxing**:
   - Roundtrip: `Value::int(n).as_int() == Some(n)` for all valid n
   - Boundary: Test i48 min/max values
   - Floats: Test NaN handling, infinities, denormals
3. **Integration tests**: All existing tests must pass
4. **Memory benchmarks**: Compare allocation counts before/after

### 1.6 Rollback Strategy

Keep `src/value.rs.bak` until all tests pass. The refactor is atomic:
either the new representation works completely, or we revert.

### 1.7 Post-Completion Notes

**Completed February 2025.** Key divergences from plan:
- Cell unification used `HeapObject::Cell(RefCell<Value>, bool)` instead of
  `cell_mask` on Closure. The bool distinguishes local (auto-unwrap) from user
  cells. This is architecturally suboptimal — metadata belongs on the consumer
  (Closure), not the data (Cell). Scheduled for fix after `value_old` removal.
- `Closure`/`JitClosure` NOT merged. Both still in `value_old`.
- `Exception` variant removed from new `HeapObject` but still in `value_old::Value`.
- `value_old` module still exists as canonical home for ~17 runtime types.
  Removal is a separate PR after old pipeline removal.

---

## Phase 2: Error System — ❌ NOT STARTED (diverged)

**Duration**: 1-2 weeks  
**Risk**: Medium  
**Dependencies**: None (can parallel with Phase 1)

### 2.1 Current State

`LError` exists in `src/error/types.rs` but is unused. The codebase uses:
```rust
pub type NativeFn = fn(&[Value]) -> Result<Value, String>;
pub type VmAwareFn = fn(&[Value], &mut VM) -> Result<Value, String>;
```

### 2.2 Target State

```rust
// src/error/types.rs - expanded
pub struct LError {
    pub kind: ErrorKind,
    pub location: Option<SourceLoc>,
    pub stack_trace: Vec<StackFrame>,
}

pub struct StackFrame {
    pub function_name: Option<String>,
    pub location: Option<SourceLoc>,
}

// src/value.rs - new function types
pub type NativeFn = fn(&[Value]) -> Result<Value, LError>;
pub type VmAwareFn = fn(&[Value], &mut VM) -> Result<Value, LError>;
```

### 2.3 Implementation Steps

#### Step 2.3.1: Expand `LError`

Add `location` and `stack_trace` fields to existing `LError`.

#### Step 2.3.2: Create error builder helpers

```rust
// src/error/builders.rs
impl LError {
    pub fn type_mismatch(expected: &str, got: &Value) -> Self {
        LError {
            kind: ErrorKind::TypeMismatch {
                expected: expected.to_string(),
                got: got.type_name().to_string(),
            },
            location: None,
            stack_trace: vec![],
        }
    }
    
    pub fn with_location(mut self, loc: SourceLoc) -> Self {
        self.location = Some(loc);
        self
    }
    
    pub fn arity(expected: usize, got: usize) -> Self { ... }
    pub fn undefined(name: &str) -> Self { ... }
    pub fn division_by_zero() -> Self { ... }
}
```

#### Step 2.3.3: Migrate primitives (mechanical)

For each file in `src/primitives/`:

```rust
// Before
pub fn prim_add(args: &[Value]) -> Result<Value, String> {
    if args.len() < 2 {
        return Err(format!("+ requires at least 2 arguments, got {}", args.len()));
    }
    // ...
}

// After
pub fn prim_add(args: &[Value]) -> Result<Value, LError> {
    if args.len() < 2 {
        return Err(LError::arity_at_least(2, args.len()));
    }
    // ...
}
```

#### Step 2.3.4: Update VM execution

```rust
// src/vm/mod.rs
impl VM {
    pub fn execute(&mut self, bytecode: &[u8]) -> Result<Value, LError> {
        // Capture stack trace on error
        match self.execute_inner(bytecode) {
            Ok(v) => Ok(v),
            Err(mut e) => {
                e.stack_trace = self.capture_stack_trace();
                Err(e)
            }
        }
    }
    
    fn capture_stack_trace(&self) -> Vec<StackFrame> {
        self.call_stack.iter().map(|frame| {
            StackFrame {
                function_name: frame.function_name.clone(),
                location: frame.source_loc.clone(),
            }
        }).collect()
    }
}
```

### 2.4 Files to Modify

| File | Changes |
|------|---------|
| `src/error/types.rs` | Add location, stack_trace fields |
| `src/error/builders.rs` | New: error constructors |
| `src/value.rs` | Change NativeFn, VmAwareFn return types |
| `src/primitives/*.rs` | ~100 functions, mechanical migration |
| `src/vm/mod.rs` | Return LError, capture stack traces |
| `src/compiler/*.rs` | Return LError from compilation |

### 2.5 Migration Script

Create a script to automate the mechanical changes:

```bash
# Find all Err(format!(...)) and Err("...".to_string())
rg 'Err\(format!\(' src/primitives/ --files-with-matches
rg 'Err\("[^"]+".to_string\(\)\)' src/primitives/ --files-with-matches
```

Most conversions follow patterns:
- `Err(format!("expected X, got {}", y))` → `Err(LError::type_mismatch("X", y))`
- `Err("division by zero")` → `Err(LError::division_by_zero())`

### 2.6 Post-Assessment Notes

**Not started as of February 2025.** The error system diverged from this plan:
- `NativeFn` returns `Result<Value, Condition>` (not `Result<Value, LError>`)
- `VmAwareFn` uses `vm.current_exception` channel, returns `LResult<Value>`
- Two error channels: `Err(String)` = VM bug (uncatchable), `vm.current_exception` = runtime (catchable)
- This design is documented in `docs/EXCEPT.md`
- The unified `LError` approach may still be worth pursuing but is not blocking.

---

## Phase 3: Syntax Type — ✅ COMPLETE

**Duration**: 2 weeks  
**Risk**: Medium  
**Dependencies**: None (can parallel)

### 3.1 Current State

The parser produces `Value`, which is also the runtime type.
Macro expansion and analysis operate on `Value`.

```
Source → Reader → Value → value_to_expr → Expr → compile → Bytecode
```

### 3.2 Target State

New `Syntax` type for pre-analysis AST:

```
Source → Lexer → Tokens → Parser → Syntax → MacroExpand → Syntax → Analyze → HIR
```

### 3.3 Syntax Definition

```rust
// src/syntax/mod.rs

/// Source location tracking
#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,  // Byte offset
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

/// Pre-analysis syntax tree
#[derive(Debug, Clone)]
pub struct Syntax {
    pub kind: SyntaxKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum SyntaxKind {
    // Atoms
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(String),       // Not interned yet
    Keyword(String),
    String(String),
    
    // Compounds
    List(Vec<Syntax>),
    Vector(Vec<Syntax>),
    
    // Quote forms (before expansion)
    Quote(Box<Syntax>),
    Quasiquote(Box<Syntax>),
    Unquote(Box<Syntax>),
    UnquoteSplicing(Box<Syntax>),
}

impl Syntax {
    /// Convert to Value for macro expansion
    pub fn to_value(&self, symbols: &mut SymbolTable) -> Value { ... }
    
    /// Convert from Value after macro expansion
    pub fn from_value(value: &Value, symbols: &SymbolTable) -> Result<Syntax, LError> { ... }
}
```

### 3.4 Implementation Steps

#### Step 3.4.1: Create `src/syntax/` module

```
src/syntax/
├── mod.rs      # Syntax, SyntaxKind
├── span.rs     # Span type
├── convert.rs  # to_value, from_value
└── display.rs  # Debug, Display
```

#### Step 3.4.2: Update parser to produce Syntax

```rust
// src/reader/parser.rs

impl Parser {
    pub fn parse(&mut self) -> Result<Syntax, LError> {
        let start = self.position();
        let kind = self.parse_kind()?;
        let end = self.position();
        Ok(Syntax {
            kind,
            span: Span { start, end, line: self.line, col: self.col },
        })
    }
    
    fn parse_kind(&mut self) -> Result<SyntaxKind, LError> {
        match self.peek()? {
            Token::LParen => self.parse_list(),
            Token::LBracket => self.parse_vector(),
            Token::Quote => {
                self.advance();
                Ok(SyntaxKind::Quote(Box::new(self.parse()?)))
            }
            Token::Int(n) => {
                self.advance();
                Ok(SyntaxKind::Int(n))
            }
            // ... etc
        }
    }
}
```

#### Step 3.4.3: Update macro expander

```rust
// src/compiler/macros.rs

pub fn expand(syntax: Syntax, macros: &MacroTable, symbols: &mut SymbolTable) 
    -> Result<Syntax, LError> 
{
    match &syntax.kind {
        SyntaxKind::List(items) if !items.is_empty() => {
            if let SyntaxKind::Symbol(name) = &items[0].kind {
                if let Some(macro_def) = macros.get(name) {
                    // Convert to Value for macro execution
                    let args: Vec<Value> = items[1..].iter()
                        .map(|s| s.to_value(symbols))
                        .collect();
                    let result = macro_def.expand(&args)?;
                    // Convert back to Syntax
                    return Syntax::from_value(&result, symbols);
                }
            }
            // Not a macro call, expand children
            let expanded: Vec<Syntax> = items.iter()
                .map(|s| expand(s.clone(), macros, symbols))
                .collect::<Result<_, _>>()?;
            Ok(Syntax { kind: SyntaxKind::List(expanded), span: syntax.span })
        }
        _ => Ok(syntax),
    }
}
```

### 3.5 Files to Modify

| File | Changes |
|------|---------|
| `src/syntax/` | New module |
| `src/reader/parser.rs` | Return Syntax instead of Value |
| `src/reader/mod.rs` | Update public API |
| `src/compiler/macros.rs` | Operate on Syntax |
| `src/compiler/converters/quasiquote.rs` | Operate on Syntax |

### 3.6 Post-Completion Notes

**Completed.** Lives at `src/syntax/` (not `src/compiler/syntax/` as planned).
Includes `Expander` for macro expansion. `Syntax` has `to_value`/`from_value`
for macro interop. Built-in macros (threading, when/unless) handled by Expander.
User-defined macros (`defmacro`) partially supported.

---

## Phase 4: HIR — ✅ COMPLETE

**Duration**: 3-4 weeks  
**Risk**: High  
**Dependencies**: Phase 3 (Syntax)

### 4.1 Purpose

HIR (High-level IR) is the first fully-analyzed representation:
- All names resolved to bindings
- All macros expanded
- Effects inferred
- Types known for literals

### 4.2 HIR Definition

```rust
// src/compiler/hir/mod.rs

/// Binding identifier (assigned during analysis)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

/// HIR expression with full analysis
#[derive(Debug, Clone)]
pub struct HirExpr {
    pub kind: HirKind,
    pub span: Span,
    pub effect: Effect,
}

#[derive(Debug, Clone)]
pub enum HirKind {
    // Literals (with types)
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Rc<str>),
    
    // Variables (fully resolved)
    Var(BindingId),
    
    // Binding forms
    Let {
        bindings: Vec<(BindingId, HirExpr)>,
        body: Box<HirExpr>,
    },
    Lambda {
        params: Vec<BindingId>,
        captures: Vec<CaptureInfo>,
        body: Box<HirExpr>,
    },
    
    // Control flow
    If {
        cond: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Box<HirExpr>,
    },
    Begin(Vec<HirExpr>),
    
    // Function application
    Call {
        func: Box<HirExpr>,
        args: Vec<HirExpr>,
        tail: bool,
    },
    
    // Mutation
    Set {
        target: BindingId,
        value: Box<HirExpr>,
    },
    Define {
        name: BindingId,
        value: Box<HirExpr>,
    },
    
    // Loops
    While {
        cond: Box<HirExpr>,
        body: Box<HirExpr>,
    },
    For {
        var: BindingId,
        iter: Box<HirExpr>,
        body: Box<HirExpr>,
    },
    
    // Exception handling
    HandlerCase {
        body: Box<HirExpr>,
        handlers: Vec<(u32, BindingId, Box<HirExpr>)>,
    },
    
    // Coroutines
    Yield(Box<HirExpr>),
}

/// Capture information for closures
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    pub binding: BindingId,
    pub from_parent: CaptureSource,
    pub is_mutated: bool,
}

#[derive(Debug, Clone)]
pub enum CaptureSource {
    Local(usize),      // Parent's local at index
    Capture(usize),    // Parent's capture at index
    Global(SymbolId),  // Global binding
}
```

### 4.3 Analyzer Implementation

```rust
// src/compiler/hir/analyze.rs

pub struct Analyzer {
    scopes: Vec<Scope>,
    bindings: HashMap<BindingId, BindingInfo>,
    next_binding_id: u32,
    effect_ctx: EffectContext,
}

struct Scope {
    bindings: HashMap<SymbolId, BindingId>,
    kind: ScopeKind,
}

enum ScopeKind {
    Global,
    Lambda { captures: Vec<CaptureInfo> },
    Let,
    Loop { var: Option<BindingId> },
}

impl Analyzer {
    pub fn analyze(&mut self, syntax: &Syntax, symbols: &SymbolTable) 
        -> Result<HirExpr, LError> 
    {
        match &syntax.kind {
            SyntaxKind::Symbol(name) => {
                let sym = symbols.lookup(name)?;
                let binding = self.resolve_var(sym)?;
                Ok(HirExpr {
                    kind: HirKind::Var(binding),
                    span: syntax.span.clone(),
                    effect: Effect::Pure,
                })
            }
            
            SyntaxKind::List(items) if self.is_let(items) => {
                self.analyze_let(items, syntax.span.clone())
            }
            
            SyntaxKind::List(items) if self.is_lambda(items) => {
                self.analyze_lambda(items, syntax.span.clone())
            }
            
            // ... other forms
        }
    }
    
    fn resolve_var(&mut self, sym: SymbolId) -> Result<BindingId, LError> {
        // Walk scopes from innermost to outermost
        for (depth, scope) in self.scopes.iter().rev().enumerate() {
            if let Some(&binding) = scope.bindings.get(&sym) {
                if depth > 0 {
                    // Captured variable - record in lambda scope
                    self.record_capture(binding, depth);
                }
                return Ok(binding);
            }
        }
        Err(LError::undefined(&symbols.name(sym)))
    }
}
```

### 4.4 Files to Create/Modify

| File | Changes |
|------|---------|
| `src/compiler/hir/mod.rs` | New: HirExpr, HirKind |
| `src/compiler/hir/analyze.rs` | New: Analyzer |
| `src/compiler/hir/effects.rs` | New: Effect inference on HIR |
| `src/compiler/hir/display.rs` | New: Debug output |
| `src/compiler/mod.rs` | Export hir module |

### 4.5 Post-Completion Notes

**Completed.** Lives at `src/hir/` (not `src/compiler/hir/`). Key difference
from plan: `HirExpr` is called `Hir`, uses `HirKind`. Tail call marking
(`is_tail` on `HirKind::Call`) is NOT implemented — hardcoded to `false` with
comment "done in a later pass." **This is a blocking defect** — the new pipeline
has no tail call optimization, causing stack overflow at depth >1000. Must be
fixed before old pipeline can be removed.

---

## Phase 5: LIR — ✅ COMPLETE

**Duration**: 3-4 weeks  
**Risk**: High  
**Dependencies**: Phase 4 (HIR)

### 5.1 Purpose

LIR (Low-level IR) is SSA form suitable for optimization and code generation.
It's close to the machine but still target-independent.

### 5.2 LIR Definition

```rust
// src/compiler/lir/mod.rs

/// Virtual register
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg(pub u32);

/// Basic block label
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label(pub u32);

/// LIR function
pub struct LirFunction {
    pub name: Option<SymbolId>,
    pub params: Vec<Reg>,
    pub blocks: Vec<BasicBlock>,
    pub entry: Label,
}

/// Basic block (no control flow within, ends with terminator)
pub struct BasicBlock {
    pub label: Label,
    pub instructions: Vec<LirInstr>,
    pub terminator: Terminator,
}

/// LIR instruction (SSA form: each Reg assigned exactly once)
pub enum LirInstr {
    // Constants
    Const { dst: Reg, value: Value },
    
    // Variable access
    LoadCapture { dst: Reg, index: usize },
    LoadGlobal { dst: Reg, sym: SymbolId },
    StoreGlobal { sym: SymbolId, src: Reg },
    
    // Closure creation
    MakeClosure { dst: Reg, func: LirFunctionId, captures: Vec<Reg> },
    
    // Function call
    Call { dst: Reg, func: Reg, args: Vec<Reg> },
    TailCall { func: Reg, args: Vec<Reg> },
    
    // Data construction
    Cons { dst: Reg, head: Reg, tail: Reg },
    MakeVector { dst: Reg, elements: Vec<Reg> },
    
    // Primitive operations (can be specialized)
    Add { dst: Reg, lhs: Reg, rhs: Reg },
    Sub { dst: Reg, lhs: Reg, rhs: Reg },
    Mul { dst: Reg, lhs: Reg, rhs: Reg },
    Div { dst: Reg, lhs: Reg, rhs: Reg },
    
    // Comparisons
    Eq { dst: Reg, lhs: Reg, rhs: Reg },
    Lt { dst: Reg, lhs: Reg, rhs: Reg },
    
    // Type checks
    IsNil { dst: Reg, src: Reg },
    IsPair { dst: Reg, src: Reg },
    
    // Cell operations
    MakeCell { dst: Reg, value: Reg },
    LoadCell { dst: Reg, cell: Reg },
    StoreCell { cell: Reg, value: Reg },
    
    // Yield (for coroutines)
    Yield { dst: Reg, value: Reg },
}

/// Block terminator
pub enum Terminator {
    Return(Reg),
    Jump(Label),
    Branch { cond: Reg, then_label: Label, else_label: Label },
    Unreachable,
}
```

### 5.3 HIR to LIR Lowering

```rust
// src/compiler/lir/lower.rs

pub struct Lowerer {
    current_block: Label,
    blocks: Vec<BasicBlock>,
    next_reg: u32,
    next_label: u32,
}

impl Lowerer {
    pub fn lower(&mut self, hir: &HirExpr) -> Reg {
        match &hir.kind {
            HirKind::Int(n) => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::Const { dst, value: Value::int(*n) });
                dst
            }
            
            HirKind::If { cond, then_branch, else_branch } => {
                let cond_reg = self.lower(cond);
                let then_label = self.fresh_label();
                let else_label = self.fresh_label();
                let join_label = self.fresh_label();
                let result = self.fresh_reg();
                
                self.terminate(Terminator::Branch {
                    cond: cond_reg,
                    then_label,
                    else_label,
                });
                
                // Then branch
                self.start_block(then_label);
                let then_result = self.lower(then_branch);
                self.emit(LirInstr::Move { dst: result, src: then_result });
                self.terminate(Terminator::Jump(join_label));
                
                // Else branch
                self.start_block(else_label);
                let else_result = self.lower(else_branch);
                self.emit(LirInstr::Move { dst: result, src: else_result });
                self.terminate(Terminator::Jump(join_label));
                
                // Join point
                self.start_block(join_label);
                result
            }
            
            HirKind::Call { func, args, tail } => {
                let func_reg = self.lower(func);
                let arg_regs: Vec<Reg> = args.iter().map(|a| self.lower(a)).collect();
                
                if *tail {
                    self.terminate(Terminator::TailCall { func: func_reg, args: arg_regs });
                    self.fresh_reg()  // Unreachable, but need a result
                } else {
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::Call { dst, func: func_reg, args: arg_regs });
                    dst
                }
            }
            
            // ... other cases
        }
    }
}
```

### 5.4 LIR to Bytecode Emission

```rust
// src/compiler/lir/emit.rs

pub struct Emitter {
    bytecode: Bytecode,
    reg_to_slot: HashMap<Reg, u8>,
    label_to_offset: HashMap<Label, usize>,
    pending_jumps: Vec<(usize, Label)>,
}

impl Emitter {
    pub fn emit(&mut self, func: &LirFunction) -> Vec<u8> {
        // First pass: assign registers to stack slots
        self.allocate_registers(func);
        
        // Second pass: emit instructions
        for block in &func.blocks {
            self.label_to_offset.insert(block.label, self.bytecode.len());
            
            for instr in &block.instructions {
                self.emit_instr(instr);
            }
            
            self.emit_terminator(&block.terminator);
        }
        
        // Third pass: fix up jumps
        self.fix_jumps();
        
        self.bytecode.into_bytes()
    }
    
    fn emit_instr(&mut self, instr: &LirInstr) {
        match instr {
            LirInstr::Const { dst, value } => {
                let idx = self.bytecode.add_constant(value.clone());
                self.bytecode.emit(Instruction::LoadConst);
                self.bytecode.emit_u16(idx);
                // Store to dst's stack slot
                let slot = self.reg_to_slot[dst];
                self.bytecode.emit(Instruction::StoreLocal);
                self.bytecode.emit_byte(slot);
            }
            // ... other instructions
        }
    }
}
```

### 5.5 Files to Create

| File | Changes |
|------|---------|
| `src/compiler/lir/mod.rs` | LIR types |
| `src/compiler/lir/lower.rs` | HIR → LIR |
| `src/compiler/lir/emit.rs` | LIR → Bytecode |
| `src/compiler/lir/optimize.rs` | Optional: constant folding, DCE |
| `src/compiler/lir/display.rs` | Debug output |

### 5.6 Post-Completion Notes

**Completed.** Lives at `src/lir/` (not `src/compiler/lir/`). The `TailCall`
instruction exists and the emitter handles it correctly. The problem is upstream:
HIR never sets `is_tail: true`. `Effect::Yields` is also not threaded through —
`emit.rs` hardcodes `Effect::Pure` (TODO comment on line 252).

---

## Phase 6: Scope System — ✅ COMPLETE (in new pipeline)

**Duration**: 2-3 weeks  
**Risk**: High  
**Dependencies**: Phase 4 (HIR), Phase 5 (LIR)

### 6.1 Current Problems

1. `VarRef` has four variants but `Local` vs `LetBound` distinction is confusing
2. Loop variables are stored in globals
3. Closure captures are static snapshots, not references
4. Runtime `ScopeStack` duplicates compile-time resolution

### 6.2 Target State

```rust
// src/binding/varref.rs - simplified

/// Fully-resolved variable reference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarRef {
    /// Local in current frame (params or let-bindings)
    Local { index: u16, is_cell: bool },
    
    /// Captured from enclosing scope
    Capture { index: u16, is_cell: bool },
    
    /// Global/top-level binding
    Global { sym: SymbolId },
}
```

### 6.3 Key Changes

#### 6.3.1: Remove `LetBound` variant

All let-bound variables become `Local` with compile-time assigned indices.

#### 6.3.2: Loop variables are locals

```rust
// For loop compilation (LIR)
HirKind::For { var, iter, body } => {
    // var is a BindingId assigned during HIR analysis
    // It compiles to a local slot, not a global
    
    let iter_reg = self.lower(iter);
    let var_slot = self.binding_to_local(var);  // Local, not global
    
    let loop_label = self.fresh_label();
    let done_label = self.fresh_label();
    
    self.start_block(loop_label);
    // Check if iter is nil
    let is_nil = self.fresh_reg();
    self.emit(LirInstr::IsNil { dst: is_nil, src: iter_reg });
    self.terminate(Terminator::Branch { 
        cond: is_nil, 
        then_label: done_label, 
        else_label: self.fresh_label() 
    });
    
    // Extract car, store in var_slot
    let car = self.fresh_reg();
    self.emit(LirInstr::Car { dst: car, src: iter_reg });
    self.emit(LirInstr::StoreLocal { slot: var_slot, src: car });
    
    // Execute body
    self.lower(body);
    
    // Advance to cdr
    let cdr = self.fresh_reg();
    self.emit(LirInstr::Cdr { dst: cdr, src: iter_reg });
    iter_reg = cdr;  // Update for next iteration
    
    self.terminate(Terminator::Jump(loop_label));
    
    self.start_block(done_label);
    // ...
}
```

#### 6.3.3: Closure captures become references for mutated variables

```rust
// During HIR analysis
fn analyze_lambda(&mut self, items: &[Syntax], span: Span) -> Result<HirExpr, LError> {
    // ...
    
    // Identify mutated captures
    let mutated_in_body = analyze_mutations(&body);
    
    for capture in &captures {
        capture.is_cell = mutated_in_body.contains(&capture.binding) 
                       || self.is_captured_and_mutated(capture.binding);
    }
    
    // ...
}

// During LIR lowering
fn lower_lambda(&mut self, params: &[BindingId], captures: &[CaptureInfo], body: &HirExpr) -> Reg {
    // For each capture, either copy value or wrap in cell
    for cap in captures {
        if cap.is_cell {
            // Capture a cell reference, not a value
            self.emit(LirInstr::LoadCaptureCell { dst, index: cap.source_index });
        } else {
            // Capture by value
            self.emit(LirInstr::LoadCapture { dst, index: cap.source_index });
        }
    }
    // ...
}
```

### 6.4 Remove Runtime ScopeStack

The VM's `ScopeStack` is no longer needed because:
- All variables resolved at compile time
- `Local` indices point directly into frame slots
- `Capture` indices point into closure environment
- `Global` uses symbol lookup

```rust
// src/vm/mod.rs - simplified
pub struct VM {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    globals: HashMap<SymbolId, Value>,
    // REMOVED: scope_stack: ScopeStack
}

pub struct CallFrame {
    ip: usize,
    bp: usize,              // Base pointer in stack
    closure: Rc<Closure>,   // Current closure (captures + constants)
}
```

### 6.5 Files to Modify

| File | Changes |
|------|---------|
| `src/binding/varref.rs` | Simplify to 3 variants |
| `src/binding/scope.rs` | Compile-time only |
| `src/vm/scope/` | Delete or minimize |
| `src/vm/mod.rs` | Remove ScopeStack |
| `src/compiler/hir/analyze.rs` | Assign local indices |
| `src/compiler/lir/emit.rs` | Emit simplified var access |

---

## Phase 7: Semantics Completion — ⚠️ PARTIAL

**Duration**: 3-4 weeks  
**Risk**: Medium  
**Dependencies**: Phases 1-6

### 7.1 Exception/Condition System

#### 7.1.1: Remove Exception variant

All error signaling uses `Condition`:

```rust
// Before: Two mechanisms
Value::Exception(exc)   // Legacy
Value::Condition(cond)  // New

// After: One mechanism
HeapObject::Condition(cond)
```

#### 7.1.2: Implement InvokeRestart

```rust
// src/compiler/bytecode.rs
pub enum Instruction {
    // ...
    InvokeRestart,  // Pop restart-id, find handler, invoke
}

// src/vm/mod.rs
fn handle_invoke_restart(&mut self) -> Result<(), LError> {
    let restart_id = self.pop()?.as_int()? as u32;
    
    // Walk handler stack looking for matching restart
    for handler in self.handler_stack.iter().rev() {
        if let Some(restart) = handler.restarts.get(&restart_id) {
            // Found it - invoke the restart function
            self.push(restart.clone());
            return self.handle_call(0);  // Call with no args
        }
    }
    
    Err(LError::runtime("No matching restart found"))
}
```

**Status**: `handler-case` complete. `handler-bind` is a stub (parsed, codegen
ignores handlers). `InvokeRestart` opcode allocated but VM handler is no-op.
`signal`/`warn`/`error` primitives are misnamed constructors that don't signal.
`try`/`catch`/`finally` is dead — excised in favor of conditions.

### 7.2 Effect System Enforcement

```rust
// src/compiler/hir/effects.rs

impl Analyzer {
    fn check_effect_compatibility(&self, caller: Effect, callee: Effect) -> Result<(), LError> {
        if !caller.permits(&callee) {
            return Err(LError::effect_violation(caller, callee));
        }
        Ok(())
    }
}

impl Effect {
    fn permits(&self, other: &Effect) -> bool {
        match (self, other) {
            (Effect::IO, _) => true,           // IO permits anything
            (Effect::Write, Effect::IO) => false,
            (Effect::Write, _) => true,        // Write permits Pure, Read, Write
            (Effect::Read, Effect::Write) | (Effect::Read, Effect::IO) => false,
            (Effect::Read, _) => true,
            (Effect::Pure, Effect::Pure) => true,
            (Effect::Pure, _) => false,
        }
    }
}
```

### 7.3 Module System

```rust
// src/module/mod.rs

pub struct Module {
    pub name: SymbolId,
    pub exports: HashSet<SymbolId>,
    pub bindings: HashMap<SymbolId, Value>,
    pub compiled: CompiledModule,
}

pub struct CompiledModule {
    pub bytecode: Vec<u8>,
    pub constants: Vec<Value>,
    pub entry_point: usize,
}

// src/compiler/hir/analyze.rs
fn analyze_import(&mut self, module_name: SymbolId) -> Result<(), LError> {
    let module = self.module_loader.load(module_name)?;
    
    // Add exported bindings to current scope
    for &exported in &module.exports {
        let binding = self.fresh_binding();
        self.current_scope_mut().bindings.insert(exported, binding);
        self.module_bindings.insert(binding, (module_name, exported));
    }
    
    Ok(())
}
```

---

## Phase 8: Cleanup — 🔄 IN PROGRESS

**Duration**: 2 weeks  
**Risk**: Low  
**Dependencies**: All previous phases

### 8.1 Dead Code Removal

Files to delete (move to `trash/`):
- `src/compiler/cranelift/phase*_milestone.rs` (13 files)
- `src/compiler/cranelift/compiler_v*.rs` (3 files)
- `src/compiler/cranelift/adaptive_compiler.rs`
- `src/compiler/cranelift/advanced_optimizer.rs`
- `src/compiler/cranelift/escape_analyzer.rs`

Total: ~22 files, ~8000 lines

### 8.2 Large File Modularization

#### 8.2.1: Split `src/compiler/compile/mod.rs` (~1200 lines)

```
src/compiler/compile/
├── mod.rs          # Compiler struct, main compile() (200 lines)
├── literal.rs      # compile_literal (100 lines)
├── variable.rs     # compile_var, compile_set (150 lines)
├── control.rs      # compile_if, compile_while, compile_for (200 lines)
├── binding.rs      # compile_let, compile_letrec, compile_define (200 lines)
├── lambda.rs       # compile_lambda (200 lines)
├── call.rs         # compile_call (100 lines)
└── utils.rs        # Existing utilities
```

#### 8.2.2: Split `src/compiler/cranelift/compiler.rs` (~1900 lines)

```
src/compiler/cranelift/
├── mod.rs          # Exports, JIT context
├── compiler.rs     # Main compile entry (300 lines)
├── expression.rs   # Expression compilation (400 lines)
├── control.rs      # Control flow (300 lines)
├── call.rs         # Function calls (300 lines)
├── closure.rs      # Closure handling (200 lines)
└── primitives.rs   # Primitive emission (300 lines)
```

#### 8.2.3: Split `src/vm/mod.rs` (~1000 lines)

Most already split into submodules. Remaining work:
```
src/vm/
├── mod.rs          # VM struct, execute loop (300 lines)
├── dispatch.rs     # Instruction dispatch table (200 lines)
└── ... (existing)
```

### 8.3 Test Suite Organization

```
tests/
├── integration/
│   ├── arithmetic.rs
│   ├── control_flow.rs
│   ├── closures.rs
│   ├── modules.rs
│   └── effects.rs
├── compiler/
│   ├── hir.rs
│   ├── lir.rs
│   └── bytecode.rs
├── value/
│   ├── nan_boxing.rs
│   └── heap.rs
└── fixtures/
    └── *.elle
```

---

## Appendices

### A. Timeline Summary

| Phase | Duration | Dependencies | Risk |
|-------|----------|--------------|------|
| 1. Value | 2-3 weeks | None | Critical |
| 2. Errors | 1-2 weeks | None | Medium |
| 3. Syntax | 2 weeks | None | Medium |
| 4. HIR | 3-4 weeks | Phase 3 | High |
| 5. LIR | 3-4 weeks | Phase 4 | High |
| 6. Scope | 2-3 weeks | Phases 4, 5 | High |
| 7. Semantics | 3-4 weeks | Phases 1-6 | Medium |
| 8. Cleanup | 2 weeks | All | Low |

**Total**: 18-25 weeks (4-6 months)

### B. Parallel Tracks

Phases 1, 2, and 3 can proceed in parallel:
- **Track A**: Value representation (Phase 1)
- **Track B**: Error system (Phase 2) + Syntax type (Phase 3)

After these converge, Phases 4-6 are sequential.

### C. Risk Mitigations

| Risk | Mitigation |
|------|------------|
| NaN-boxing breaks everything | Branch; comprehensive tests before/after; keep backup |
| HIR/LIR bugs | Incremental: HIR works completely before starting LIR |
| Scope changes break code | Run full test suite after each sub-phase |
| Timeline slips | Prioritize correctness; defer optimization to 3.0 |

### D. Success Criteria

Phase is complete when:
1. All existing tests pass
2. No performance regression >10%
3. Code review approved
4. Documentation updated

### E. Files Changed Summary

| Category | Count |
|----------|-------|
| New files | ~30 |
| Deleted files | ~22 |
| Heavily modified | ~40 |
| Lightly modified | ~60 |
| Unchanged | ~50 |

---

**End of Implementation Plan**
