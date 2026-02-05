# Elle Lisp Interpreter - Roadmap

## Current Status

**Elle** is a high-performance bytecode-compiled Lisp interpreter written in Rust at **v1.0.0 production maturity** with all core features implemented and extensively tested.

- **Current Version**: v1.0.0 (Production Ready)
- **Test Coverage**: 1,104 tests passing (159 lib + 943 integration + 2 doc)
- **Build Status**: All clippy warnings resolved (0 warnings)

---

## Roadmap - Unimplemented Features

### High Priority

1. **Variable Binding in Patterns**
   - Status: Not started
   - Example: `(match (list 1 2) ((x y) (+ x y)))` → 3
   - Impact: Would enable destructuring in pattern matching
   - Complexity: Medium
   - Tests needed: ~15

2. **Module-Qualified Names**
   - Status: Not started
   - Example: `(list:length (list 1 2 3))` → 3
   - Impact: Cleaner namespace disambiguation without imports
   - Complexity: Medium
   - Tests needed: ~10

3. **Full Quasiquote Evaluation**
   - Status: Partial (basic expansion works)
   - Current: Basic parameter substitution
   - Missing: Proper `` `(,x ,@y) `` expansion with nested quoting
   - Complexity: High
   - Tests needed: ~20

4. **JIT Compilation**
   - Status: Not started
   - Target: 5-10x speedup on hot paths
   - Approach: Profile bytecode, compile to native code
   - Complexity: Very High
   - Estimated effort: 3-4 months

### Medium Priority

1. **Callback Support (FFI)**
   - Status: Not started
   - Feature: Pass Elle functions as C function pointers
   - Example: `(set-callback! lib-handle "on-event" event-handler)`
   - Complexity: High
   - Tests needed: ~10

2. **Package Manager**
   - Status: Not started
   - Features: Registry, dependency resolution, versioning
   - Complexity: High
   - Estimated effort: 2-3 months

3. **Async/Await Syntax**
   - Status: Not started
   - Current: Thread spawning available but no async syntax
   - Planned: `(async (await promise))`
   - Complexity: High
   - Tests needed: ~20

4. **Type System (Optional)**
   - Status: Not started
   - Scope: Optional type annotations, inference
   - Example: `(define (add [x :int] [y :int]) :int (+ x y))`
   - Complexity: Very High
   - Estimated effort: 2-3 months

5. **REPL Debugger**
   - Status: Not started
   - Features: Step through code, breakpoints, inspect frames
   - Complexity: High
   - Tests needed: ~15

### Low Priority (Out of Scope)

1. **Object-Oriented Features** - Classes, inheritance, polymorphism
2. **Advanced Meta-Programming** - Full macro hygiene, syntax analysis
3. **Distributed Execution** - Multi-machine execution
4. **Real-Time Constraints** - Hard real-time guarantees
5. **WASM Support** - Browser-based Elle execution

---

## Known Limitations in Current Implementation

### Language
- Pattern matching limited to literals, wildcards, nil (no variable binding)
- Macros use basic expansion with parameter substitution
- Module names cannot be used with `:` syntax (must use imports)

### FFI
- Cannot pass Elle functions as C callbacks
- Advanced type marshaling requires manual handling
- No automatic binding generation from C headers

### Performance
- Bytecode interpreted, not JIT compiled (~10x slower than native Rust)
- Value cloning overhead for large lists
- 20-30% bytecode dispatch overhead per instruction

### Platform
- macOS and Windows support untested recently
- WASM not supported

---

## Implementation Notes for Developers

### Variable Binding in Patterns
- Requires extending pattern AST to include binding variables
- Modify `src/compiler/converters.rs` match expression handling
- Update `src/vm/mod.rs` to bind variables in pattern context
- No new bytecode instructions needed

### Module-Qualified Names
- Parser needs to recognize `symbol:symbol` syntax
- Add resolution in compiler symbol lookup
- Extend scope tracking to search qualified namespaces
- Backward compatible with current import mechanism

### JIT Compilation
- Start with profiling instrumentation to identify hot paths
- Build incremental JIT with cranelift or LLVM backend
- Need to decide on bytecode format for JIT compatibility
- Consider Lua's approach (LuaJIT) for reference

### Callback Support
- Requires storing Elle function pointers as opaque C void*
- Need marshaling layer to convert C calls back to Elle
- Stack unwinding must be safe across language boundary

---

## Contributing

Help wanted in these areas:

- **Implementing High Priority items** - Start with Variable Binding in Patterns
- **FFI Bindings** - Wrappers for GTK4, SDL2, LLVM, etc.
- **Examples** - Real-world programs demonstrating current features
- **Documentation** - Guides for implementing unfinished features
- **Testing** - Platform testing on macOS, Windows

See `CONTRIBUTING.md` for development setup.

---

Last Updated: February 5, 2026
Status: Production Ready (v1.0.0)
