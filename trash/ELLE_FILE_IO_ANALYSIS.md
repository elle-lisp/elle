# Elle File I/O Support Analysis

## Executive Summary

Elle currently has **no dedicated file I/O primitives**, but the infrastructure is well-designed to support adding them. File operations are accessible through the FFI system (e.g., calling C's `open`, `read`, `write`), and the existing module loading system uses `std::fs::read_to_string()` for file operations. Adding native file I/O support would be straightforward given the modular primitive architecture.

---

## 1. Current File I/O Capabilities

### Limited Built-in Support
- **`import-file`** (src/primitives.rs:423-477): Loads and compiles `.elle` files as modules using `std::fs::read_to_string()`
  - Already performs file I/O internally
  - Returns `true` on success, error message on failure
  - Prevents circular dependencies

- **`add-module-path`** (src/primitives.rs:479-503): Adds directories to module search paths
  - Manages filesystem paths but doesn't perform I/O itself

### Indirect File I/O via FFI
- Users can call C library functions through the FFI system:
  ```lisp
  (load-library "libc.so.6")
  ; Then call fopen, read, write, close, etc.
  ```
- Requires manual type marshaling and error handling

### No High-Level File Functions
Missing primitives for:
- `open-file`, `close-file`, `read-file`, `write-file`
- `read-line`, `write-line`, `append-to-file`
- `file-exists?`, `delete-file`, `rename-file`
- `list-directory`, `make-directory`
- `read-all`, `write-string`

---

## 2. Primitive System Architecture

### How Primitives Are Structured

**File Structure:**
```
src/primitives/
├── mod.rs          # Exports and registration (536 lines)
├── arithmetic.rs   # Math operations
├── string.rs       # String manipulation
├── list.rs         # List operations
├── vector.rs       # Vector operations
├── comparison.rs   # Comparisons
├── exception.rs    # Exception handling
├── concurrency.rs  # Threading primitives
├── debug.rs        # Debugging utilities (uses std::fs internally)
├── meta.rs         # Meta-programming
├── type_check.rs   # Type predicates
├── higher_order.rs # map, filter, fold
└── math.rs         # Math functions
```

Total: ~1,278 lines across all primitive modules

### Primitive Registration Pattern

**From `src/primitives.rs`:**

1. **Define a primitive function** with signature:
   ```rust
   pub fn prim_my_function(args: &[Value]) -> Result<Value, String>
   ```

2. **Register it globally** in `register_primitives()`:
   ```rust
   pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) {
       register_fn(vm, symbols, "my-function", prim_my_function);
   }
   ```

3. **The `register_fn` helper** (line 261-269):
   ```rust
   fn register_fn(
       vm: &mut VM,
       symbols: &mut SymbolTable,
       name: &str,
       func: fn(&[Value]) -> Result<Value, String>,
   ) {
       let sym_id = symbols.intern(name);
       vm.set_global(sym_id.0, Value::NativeFn(func));
   }
   ```

### Key Design Patterns

1. **Error Handling**: All primitives return `Result<Value, String>` for consistent error propagation
2. **Type Checking**: Primitives validate argument types and counts early
3. **Value Conversion**: Helper methods like `args[0].as_int()`, `args[0].list_to_vec()` handle conversions
4. **Module Organization**: Related functions grouped in separate modules with clear responsibilities

### Example: Simple Primitive (string-length)
```rust
// src/primitives/string.rs:6-14
pub fn prim_string_length(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string-length requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(s) => Ok(Value::Int(s.len() as i64)),
        _ => Err("string-length requires a string".to_string()),
    }
}
```

---

## 3. FFI System Architecture

### FFI Structure
```
src/ffi/
├── mod.rs                      # FFISubsystem manager
├── bindings.rs                 # Symbol binding infrastructure
├── call.rs                     # Function calling mechanism
├── callback.rs                 # C→Elle callbacks
├── header.rs                   # C header parsing
├── loader.rs                   # Dynamic library loading
├── marshal.rs                  # Type marshaling (Elle ↔ C)
├── memory.rs                   # Memory management
├── safety.rs                   # Safety checks
├── symbol.rs                   # Symbol resolution
├── types.rs                    # C type definitions
├── wasm.rs                     # WebAssembly support (stubs)
└── primitives/
    ├── mod.rs                  # FFI primitive exports
    ├── calling.rs              # call-c-function implementation
    ├── library.rs              # load-library, list-libraries
    ├── callbacks.rs            # make-c-callback, free-callback
    ├── enums.rs                # define-enum, load-header-with-lib
    ├── context.rs              # VM context management
    ├── memory.rs               # Memory registration and stats
    ├── types.rs                # Type parsing
    ├── wrappers.rs             # Wrapper re-exports
    └── calling.rs              # Function calling primitives
```

### FFI Primitive Registration Pattern

**Two-function approach** for context-aware operations (from `src/ffi/primitives/library.rs`):

1. **Core function** (takes VM reference):
   ```rust
   pub fn prim_load_library(vm: &mut VM, args: &[Value]) -> Result<Value, String> {
       let path = match &args[0] {
           Value::String(s) => s.as_ref(),
           _ => return Err("load-library requires a string path".to_string()),
       };
       let lib_id = vm.ffi_mut().load_library(path)?;
       Ok(Value::LibHandle(LibHandle(lib_id)))
   }
   ```

2. **Wrapper function** (uses thread-local VM context):
   ```rust
   pub fn prim_load_library_wrapper(args: &[Value]) -> Result<Value, String> {
       let path = match &args[0] {
           Value::String(s) => s.as_ref(),
           _ => return Err("load-library requires a string path".to_string()),
       };
       let vm_ptr = super::context::get_vm_context().ok_or("FFI not initialized")?;
       unsafe {
           let vm = &mut *vm_ptr;
           let lib_id = vm.ffi_mut().load_library(path)?;
           Ok(Value::LibHandle(LibHandle(lib_id)))
       }
   }
   ```

3. **Registration** (primitives.rs:147-225):
   ```rust
   register_fn(vm, symbols, "load-library", ffi_primitives::prim_load_library_wrapper);
   ```

### VM Context Management

**From `src/ffi/primitives/context.rs`:**
```rust
thread_local! {
    static VM_CONTEXT: RefCell<Option<*mut VM>> = const { RefCell::new(None) };
}

pub fn set_vm_context(vm: *mut VM) { ... }
pub fn get_vm_context() -> Option<*mut VM> { ... }
pub fn clear_vm_context() { ... }
```

**Used in tests** (src/tests/integration/advanced.rs):
```rust
fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    
    ffi_primitives::set_vm_context(&mut vm as *mut VM);
    let result = vm.execute(&bytecode);
    ffi_primitives::clear_vm_context();
    
    result
}
```

---

## 4. Value System

### Value Enum (src/value.rs:115-133)
```rust
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(SymbolId),
    String(Rc<str>),
    Cons(Rc<Cons>),           // Lisp lists
    Vector(Rc<Vec<Value>>),
    Closure(Rc<Closure>),
    NativeFn(NativeFn),
    LibHandle(LibHandle),      // For FFI
    CHandle(CHandle),          // For FFI (opaque C pointers)
    Exception(Rc<Exception>),
}
```

### Opaque File Handle Pattern
Could use `CHandle` for file descriptors:
```rust
// File handles could be represented as CHandle wrapping FILE*
let handle = Value::CHandle(CHandle::new(file_ptr as *const c_void, file_id));
```

---

## 5. Module System

### Module Definition (src/symbol.rs)
```rust
pub struct ModuleDef {
    pub name: SymbolId,
    pub exports: Vec<SymbolId>,
}
```

### Module Registration (src/primitives.rs:299-357)
Modules organize related functions:
- **list module**: length, append, reverse, map, filter, fold, nth, last, take, drop
- **string module**: string-length, string-append, string-upcase, string-downcase, substring, etc.
- **math module**: +, -, *, /, mod, abs, sqrt, sin, cos, tan, log, exp, pow, floor, ceil, round, pi, e

Pattern for file I/O module:
```rust
fn init_file_module(vm: &mut VM, symbols: &mut SymbolTable) {
    let mut file_exports = std::collections::HashMap::new();
    
    let functions = vec![
        "open-file", "close-file", "read-file", "write-file",
        "read-line", "write-line", "file-exists?", "delete-file"
    ];
    
    // ... populate exports ...
    
    let file_module = ModuleDef {
        name: symbols.intern("file"),
        exports,
    };
    symbols.define_module(file_module);
    vm.define_module("file".to_string(), file_exports);
}
```

---

## 6. Files That Would Need Modification/Creation

### Files to Create
1. **`src/primitives/file_io.rs`** (NEW - ~300-400 lines)
   - Core file I/O primitives
   - `open-file`, `close-file`, `read-file`, `write-file`
   - `read-line`, `write-line`, `append-to-file`
   - `file-exists?`, `delete-file`, `rename-file`

2. **`tests/integration/file_io.rs`** (NEW - ~200-300 lines)
   - Integration tests for file operations
   - Temporary file handling
   - Error cases

### Files to Modify
1. **`src/primitives.rs`** (+30-50 lines)
   - Import the new `file_io` module: `pub mod file_io;`
   - Import functions: `use self::file_io::*;`
   - Register functions in `register_primitives()`:
     ```rust
     register_fn(vm, symbols, "open-file", prim_open_file);
     register_fn(vm, symbols, "close-file", prim_close_file);
     // ... etc
     ```
   - Add `init_file_module()` call in `init_stdlib()`
   - Define module exports

2. **`src/value.rs`** (OPTIONAL - ~20-30 lines)
   - Add `FileHandle` struct similar to `LibHandle`:
     ```rust
     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
     pub struct FileHandle(pub u32);
     ```
   - Add variant to Value enum: `FileHandle(FileHandle)`
   - Update `PartialEq` implementation for Value
   - Add `type_name()` method for "file-handle"

3. **`src/vm/core.rs`** (OPTIONAL - ~30-50 lines)
   - If using handles: add file handle table to VM struct
   - Methods: `vm.allocate_file_handle(file_ptr)`, `vm.get_file_handle(id)`

4. **`tests/integration/mod.rs`** (+1 line)
   - Add `mod file_io;` to module declarations

### Optional Enhancements
- **`src/ffi/primitives/file_io.rs`**: FFI-based file I/O (calling C fopen/fread)
- **Better handle management**: File handle registry similar to FFI library handles
- **Stream abstraction**: Wrapper around FILE* or file descriptor

---

## 7. Design Patterns and Lessons from Existing Code

### Error Handling Pattern
All primitives follow this pattern:
```rust
pub fn prim_function(args: &[Value]) -> Result<Value, String> {
    // 1. Validate argument count
    if args.len() != expected {
        return Err(format!("function: expected {} arguments, got {}", expected, args.len()));
    }
    
    // 2. Extract and validate argument types
    let arg1 = match &args[0] {
        Value::ExpectedType(v) => v,
        _ => return Err(format!("function: argument 1 must be ExpectedType, got {}", args[0].type_name())),
    };
    
    // 3. Perform operation with proper error handling
    operation()
        .map_err(|e| format!("function: operation failed: {}", e))
        .map(|result| Value::ResultType(result))
}
```

### Rust Safety with File Operations
```rust
// Use std::fs for safety
use std::fs;

pub fn prim_read_file(args: &[Value]) -> Result<Value, String> {
    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("read-file requires a string path".to_string()),
    };
    
    // Rust's fs module handles all safety concerns
    fs::read_to_string(path)
        .map(|content| Value::String(Rc::from(content)))
        .map_err(|e| format!("read-file: {}", e))
}
```

### Pattern Matching Example (from `debug.rs`)
```rust
#[cfg(target_os = "linux")]
fn platform_specific_operation() { ... }

#[cfg(target_os = "macos")]
fn platform_specific_operation() { ... }

#[cfg(target_os = "windows")]
fn platform_specific_operation() { ... }
```

---

## 8. Current Limitations and Workarounds

### Current Limitation: No Binary File Support
All current string operations assume UTF-8. File I/O would need to handle:
- Binary data (vectors of bytes)
- Different encodings
- Large files (streaming needed)

### Current Limitation: Synchronous Only
All operations are synchronous (blocking). Async I/O would require:
- Different Value type (Promise/Future)
- Async runtime integration
- More complex error handling

### Current Limitation: No Stream Abstraction
Module system uses `read_to_string()` which loads entire file. For large files:
- Need iterator protocol
- Or read(size) primitives
- Or buffered reader abstraction

### Workaround: FFI + libc
Users can already do file I/O via FFI:
```lisp
(load-library "libc.so.6")
; Call fopen, fread, fwrite, fclose through FFI
```

---

## 9. Integration Testing Pattern

**From `tests/integration/advanced.rs`:**

```rust
use elle::compiler::value_to_expr;
use elle::ffi_primitives;
use elle::{compile, read_str, register_primitives, SymbolTable, Value, VM};

fn eval(input: &str) -> Result<Value, String> {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    
    // Required for FFI/module loading
    ffi_primitives::set_vm_context(&mut vm as *mut VM);
    
    let value = read_str(input, &mut symbols)?;
    let expr = value_to_expr(&value, &mut symbols)?;
    let bytecode = compile(&expr);
    let result = vm.execute(&bytecode);
    
    ffi_primitives::clear_vm_context();
    result
}

#[test]
fn test_file_operation() {
    assert!(eval("(read-file \"test.txt\")").is_ok());
}
```

---

## 10. Summary of Implementation Steps

### Phase 1: Basic File Reading (Simple)
**Files modified: 2 (src/primitives.rs, new src/primitives/file_io.rs)**

```rust
// src/primitives/file_io.rs
pub fn prim_read_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 { return Err("...".to_string()); }
    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("...".to_string()),
    };
    std::fs::read_to_string(path)
        .map(|c| Value::String(Rc::from(c)))
        .map_err(|e| e.to_string())
}

pub fn prim_write_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 { return Err("...".to_string()); }
    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("...".to_string()),
    };
    let content = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("...".to_string()),
    };
    std::fs::write(path, content)
        .map(|_| Value::Nil)
        .map_err(|e| e.to_string())
}

pub fn prim_file_exists(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 { return Err("...".to_string()); }
    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("...".to_string()),
    };
    Ok(Value::Bool(std::path::Path::new(path).exists()))
}
```

### Phase 2: File Handles (Moderate)
**Files modified: 4 (src/primitives.rs, src/primitives/file_io.rs, src/value.rs, src/vm/core.rs)**

- Add FileHandle to Value enum
- Implement open-file, close-file
- Add file handle registry to VM
- Streaming read/write operations

### Phase 3: Advanced Features (Complex)
**Files modified: 5+ (all above plus new FFI primitives)**

- Directory operations (list-directory, make-directory)
- Path manipulation (absolute-path, directory-name)
- Permissions and metadata
- FFI-based file I/O alternatives

---

## 11. Key Takeaways

1. **Architecture is Ready**: Primitives are modular, registration is simple, error handling is consistent
2. **No Major Obstacles**: File I/O fits naturally into existing design
3. **Two Implementation Styles**:
   - **Simple**: Use `std::fs` directly (safe, Rust-like)
   - **Advanced**: File handles + registry (more flexible)
4. **Testing Infrastructure**: Integration tests are well-established
5. **FFI Alternative**: Already possible to use C file functions
6. **Next Steps**: Create `src/primitives/file_io.rs` with 5-10 core functions, register them, add tests

---

## 12. Code Locations Quick Reference

| Component | File | Lines | Key Items |
|-----------|------|-------|-----------|
| Primitive Registry | src/primitives.rs | 536 | register_primitives(), register_fn() |
| String Primitives | src/primitives/string.rs | 165 | Pattern for simple primitives |
| FFI Library Primitives | src/ffi/primitives/library.rs | 72 | Wrapper pattern, context usage |
| Module System | src/primitives.rs:299-357 | 60 | Module registration pattern |
| Value Enum | src/value.rs:115-133 | 20 | Current Value variants |
| FFI Context | src/ffi/primitives/context.rs | 32 | Thread-local context management |
| Integration Tests | tests/integration/advanced.rs | 120+ | eval() pattern, VM setup |
| Module Loading | src/primitives.rs:423-477 | 55 | Existing file I/O usage |
