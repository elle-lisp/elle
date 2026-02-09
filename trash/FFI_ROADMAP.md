# Elle FFI (Foreign Function Interface) Roadmap

## Vision

Enable Elle to seamlessly call C/C++ libraries like GTK4, SDL2, and LLVM, making it possible to build graphical applications, games, and compiler infrastructure directly from Elle Lisp. Support both Linux native execution and WASM compilation for browser-based applications.

## User Story

```
1. User opens Elle REPL or loads a script
2. User loads a shared library (.so on Linux, .wasm module with native fallback)
   (load-library "gtk-4.so" :header "gtk.h")
3. User calls functions with proper type conversion
   (gtk-init! argc argv)
   (let window (gtk-window-new GTK_WINDOW_TOPLEVEL))
4. User defines and marshals complex types (structs, callbacks)
   (define-c-struct GtkWidget ...)
   (define-callback on-destroy (fn [widget data] ...))
5. User passes Elle values as C arguments
   (g-signal-connect! window "destroy" on-destroy nil)
6. User receives properly typed results
   (let width (gtk-widget-get-allocated-width window))
```

---

## Design Philosophy

### Principles

1. **Type Safety First** - Static type checking for FFI calls, catch type errors early
2. **Zero Copy Where Possible** - Minimize marshaling overhead for performance
3. **Graceful Degradation** - Work on both native (Linux) and WASM with fallbacks
4. **Header Driven** - Parse C headers to auto-generate bindings (using bindgen)
5. **Lisp-First API** - Make FFI feel natural in Lisp, not bolted-on
6. **Sandbox Capable** - WASM target requires safe isolation of C code
7. **GC Integration** - Coordinate with Elle's memory management

### Non-Goals

- Full C++ template support (handle simple cases, not complex template metaprogramming)
- Arbitrary machine code generation
- Inline assembly support
- Real-time constraint guarantees
- Full POSIX signal handling

---

## Architecture

### High-Level Execution Flow

```
Elle Code
    ↓
(load-library "gtk-4.so" :header "gtk.h")
    ↓
FFI Loader
  ├─ Bindgen (parse gtk.h)
  ├─ Generate type definitions
  └─ Load gtk-4.so dynamically
    ↓
Generated Lisp Bindings + Function Stubs
    ↓
(gtk-window-new GTK_WINDOW_TOPLEVEL)
    ↓
Type Marshaler
  ├─ Validate arg types match signature
  ├─ Convert Elle values → C representation
  └─ Prepare stack/registers for C call
    ↓
libffi Call Wrapper
  ├─ Invoke C function
  ├─ Handle calling convention
  └─ Catch C-side errors
    ↓
Result Marshaler
  ├─ Convert C return value → Elle Value
  └─ Handle pointers (struct refs, callbacks)
    ↓
Elle Result Value
```

### Module Structure

```
src/ffi/
├── mod.rs                 # FFI subsystem integration
├── loader.rs              # Dynamic library loading (libloading)
├── symbol.rs              # Symbol resolution and caching
├── types.rs               # C type system (int, float, struct, etc.)
├── marshal.rs             # Value marshaling (Elle ↔ C)
├── call.rs                # libffi wrapper for function calls
├── header.rs              # Header file handling (bindgen integration)
├── bindings.rs            # Auto-generated binding compiler
├── callback.rs            # C callbacks → Lisp closures
├── memory.rs              # Memory management and GC coordination
├── wasm.rs                # WASM-specific code paths
└── safety.rs              # Safety checks and error handling
```

### Value Representation Strategy

Elle's Value type will be extended to support FFI:

```rust
pub enum Value {
    // ... existing types ...
    
    // FFI types
    CHandle(CValue),              // Opaque C pointer (e.g., GtkWidget*)
    CStruct(CStructValue),        // Structured C data (GtkAllocation, etc.)
    CEnum(i32),                   // C enum value with type info
    LibraryHandle(LibHandle),     // Handle to loaded .so
}

pub struct CValue {
    pub ptr: *const c_void,
    pub type_id: TypeId,
    pub type_name: String,        // "GtkWidget", "SDL_Renderer", etc.
    pub is_owned: bool,           // Does Elle own this memory?
}

pub struct CStructValue {
    pub data: Vec<u8>,            // Raw struct bytes
    pub type_id: TypeId,
    pub layout: StructLayout,     // Field offsets/sizes
}
```

---

## Phase 1: Core FFI Infrastructure (Weeks 1-3)

### Goals
- Load shared libraries dynamically
- Resolve and cache function symbols
- Call simple C functions (no structs)
- Support basic types: int, float, double, char, bool, void

### Deliverables

#### 1.1 Dynamic Library Loading (`src/ffi/loader.rs`)

```rust
pub struct LibraryHandle {
    id: u32,
    path: String,
    native: Option<libloading::Library>,  // Linux
    wasm_module: Option<WasmModule>,      // WASM
}

impl VM {
    pub fn load_library(&mut self, path: &str) -> Result<LibHandle, String> {
        // Load .so on Linux
        // Load .wasm module on WASM targets
        // Fail gracefully if both unavailable
    }
}
```

**Primitives:**
- `(load-library path &key header)` → library-handle
- `(get-library-function lib-handle name)` → function-handle

**Tests:**
- Load system libc
- Load custom test .so
- Handle missing files
- Handle symbol resolution failures

#### 1.2 Type System (`src/ffi/types.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CType {
    Void,
    Bool,
    Char, UChar,
    Short, UShort,
    Int, UInt,
    Long, ULong,
    LongLong, ULongLong,
    Float, Double, LongDouble,
    Pointer(Box<CType>),           // *int, **char, etc.
    Struct(StructId),
    Enum(EnumId),
    Array(Box<CType>, usize),      // int[10]
}

pub struct FunctionSignature {
    pub name: String,
    pub args: Vec<CType>,
    pub return_type: CType,
    pub variadic: bool,            // For printf, sprintf, etc.
}
```

**Type Size/Alignment Calculation:**
- Platform-aware (x86-64 Linux layout differs from WASM)
- Respect struct packing directives
- Handle pointer size differences (64-bit vs WASM 32-bit)

**Tests:**
- Size calculations for all basic types
- Platform-specific layout verification
- Alignment edge cases

#### 1.3 Function Call Wrapper (`src/ffi/call.rs`)

```rust
pub struct FunctionCall {
    func: *const c_void,
    signature: FunctionSignature,
    cif: ffi_cif,                  // libffi calling interface
}

impl FunctionCall {
    pub fn call(&self, args: &[Value]) -> Result<Value, String> {
        // Type check args against signature
        // Marshal Elle Values → C representation
        // Call function via libffi
        // Marshal C return value → Elle Value
        // Handle errors
    }
}
```

**Calling Conventions:**
- Linux x86-64: System V AMD64 ABI
- WASM: Single linear memory model
- Proper register allocation via libffi

**Tests:**
- Call simple functions (strlen, abs, sin, etc.)
- Verify return values
- Test all basic types
- Benchmark call overhead

### Dependencies to Add

```toml
libloading = "0.8"       # Dynamic library loading
libffi-sys = "0.4"       # FFI calling conventions (bindgen auto-generated)
```

### Implementation Notes

- On WASM, dlopen/dlsym won't work; need to link symbols at compile time or use js-sys to call browser APIs
- Linux native path: full dynamic loading support
- Start with Linux, add WASM stubs that error gracefully
- Cache loaded libraries to avoid repeated loads

---

## Phase 2: Type Marshaling and Structs (Weeks 4-6)

### Goals
- Marshal complex types (structs, arrays, pointers)
- Define custom C structs in Elle
- Pass and return struct values
- Support nested structs

### Deliverables

#### 2.1 Marshaling Engine (`src/ffi/marshal.rs`)

```rust
pub trait CMarshaler {
    fn elle_to_c(&self, value: &Value) -> Result<*const c_void, String>;
    fn c_to_elle(&self, ptr: *const c_void) -> Result<Value, String>;
    fn c_type(&self) -> CType;
}

impl CMarshaler for IntMarshaler {
    // Handle Elle Int → C int conversion
}

impl CMarshaler for StructMarshaler {
    // Handle Elle CStruct → C struct (memcpy)
    // Handle C struct → Elle CStruct (deserialize)
}

impl CMarshaler for StringMarshaler {
    // Handle Elle String → C char* (null-terminated)
    // Handle C char* → Elle String
}

impl CMarshaler for ArrayMarshaler {
    // Handle Elle Vector → C array
    // Handle C array → Elle Vector
}
```

**Marshaling Paths:**

| Elle → C | Strategy | Cost |
|----------|----------|------|
| Int/Float | Direct cast | 0ns |
| Bool | bool = (val != 0) | ~1ns |
| String | Rc → *char | ~5ns (create temp buffer) |
| Vector | Vec → *T array | ~10ns (copy) |
| CStruct | CStructValue → memcpy | ~50ns |
| CHandle | Opaque pointer pass-through | ~1ns |

#### 2.2 Struct Definition and Layout (`src/ffi/types.rs`)

```rust
pub struct StructLayout {
    id: StructId,
    name: String,
    fields: Vec<StructField>,
    size: usize,
    align: usize,
    offsets: Vec<usize>,          // Byte offset of each field
}

pub struct StructField {
    name: String,
    ctype: CType,
    offset: usize,
}

// In Elle:
(define-c-struct GtkAllocation
  (x :int)
  (y :int)
  (width :int)
  (height :int))
```

**Struct Size Calculation:**
- Respect C alignment rules
- Handle padding
- Support #pragma pack directives (future)
- Cross-platform (Linux vs WASM alignment may differ)

#### 2.3 Struct Instance Creation and Access

```rust
pub struct CStructValue {
    layout: StructLayout,
    data: Vec<u8>,                // Raw bytes
}

// In Elle:
(let alloc (make-struct GtkAllocation))
(struct-set! alloc 'x 10)
(let x (struct-get alloc 'x))  ; => 10
```

**Primitives:**
- `(define-c-struct name fields)` → struct-id
- `(make-struct struct-id &rest initial-values)` → cstruct-value
- `(struct-get struct-value field-name)` → value
- `(struct-set! struct-value field-name value)` → struct-value
- `(struct-size struct-id)` → byte-count

**Tests:**
- Define structs with various field types
- Create instances
- Access/modify fields
- Nested struct support
- Pass structs to C functions
- Receive struct results from C

### Dependencies

```toml
memcpy, memset already available via libc crate
```

---

## Phase 3: Header File Parsing and Auto-Binding (Weeks 7-10)

### Goals
- Parse C headers with bindgen
- Auto-generate Elle bindings
- Support all GTK4/SDL2/LLVM types
- Handle #define macros and enums

### Deliverables

#### 3.1 Header Parser Integration (`src/ffi/header.rs`)

```rust
pub struct HeaderParser {
    include_paths: Vec<PathBuf>,
    defines: HashMap<String, String>,
    parsed_cache: HashMap<String, ParsedHeader>,
}

pub struct ParsedHeader {
    pub types: HashMap<String, ParsedType>,
    pub functions: HashMap<String, FunctionSignature>,
    pub constants: HashMap<String, i64>,
    pub enums: HashMap<String, EnumDef>,
}

pub enum ParsedType {
    Struct(StructLayout),
    Enum(EnumDef),
    Typedef { name: String, base: CType },
    OpaquePointer { name: String },
}

impl HeaderParser {
    pub fn parse(&mut self, header_path: &str) -> Result<ParsedHeader, String> {
        // Use bindgen to parse header
        // Extract types, functions, constants
        // Cache for reuse
    }
}
```

**Bindgen Integration:**
- Run bindgen as build step or at runtime
- Parse GTK4 headers → Elle bindings
- Parse SDL2 headers → Elle bindings
- Parse LLVM headers → Elle bindings
- Handle header dependencies (#include)

#### 3.2 Binding Generator (`src/ffi/bindings.rs`)

```rust
pub fn generate_elle_bindings(parsed: &ParsedHeader) -> String {
    let mut lisp_code = String::new();
    
    // Generate struct definitions
    for (name, type_def) in &parsed.types {
        if let ParsedType::Struct(layout) = type_def {
            lisp_code.push_str(&format!(
                "(define-c-struct {} ...)\n",
                name
            ));
        }
    }
    
    // Generate enum definitions
    for (name, enum_def) in &parsed.enums {
        lisp_code.push_str(&format!(
            "(define-enum {} {:?})\n",
            name, enum_def.variants
        ));
    }
    
    // Generate function wrappers
    for (name, sig) in &parsed.functions {
        lisp_code.push_str(&generate_function_wrapper(name, sig));
    }
    
    // Generate constants
    for (name, value) in &parsed.constants {
        lisp_code.push_str(&format!(
            "(define {} {})\n",
            name, value
        ));
    }
    
    lisp_code
}

fn generate_function_wrapper(name: &str, sig: &FunctionSignature) -> String {
    format!(
        "(define ({} {:?}) (ffi-call '{}' {:?}))\n",
        name, sig.args, name, sig.return_type
    )
}
```

**Example Generated Binding (gtk4.h → gtk4.lisp):**

```lisp
; Auto-generated from gtk4.h

(define-c-struct GtkAllocation
  (x :int)
  (y :int)
  (width :int)
  (height :int))

(define GTK_WINDOW_TOPLEVEL 0)
(define GTK_WINDOW_POPUP 1)

(define (gtk-init! argc argv)
  (ffi-call 'gtk-4.so' "gtk_init" (:pointer :pointer) :void 
    argc argv))

(define (gtk-window-new window-type)
  (ffi-call 'gtk-4.so' "gtk_window_new" (:int) :pointer
    window-type))

(define (gtk-widget-get-allocated-width widget)
  (ffi-call 'gtk-4.so' "gtk_widget_get_allocated_width" 
    (:pointer) :int
    widget))

(define (gtk-window-set-title window title)
  (ffi-call 'gtk-4.so' "gtk_window_set_title" 
    (:pointer :string) :void
    window title))
```

#### 3.3 Loading and Using Generated Bindings

```rust
pub fn load_header_with_library(
    vm: &mut VM,
    header_path: &str,
    library_path: &str,
) -> Result<(), String> {
    // 1. Load library
    let lib_handle = vm.load_library(library_path)?;
    
    // 2. Parse header
    let parsed = HeaderParser::new().parse(header_path)?;
    
    // 3. Generate bindings
    let lisp_code = generate_elle_bindings(&parsed);
    
    // 4. Evaluate bindings in VM
    // (This uses existing Elle eval)
    vm.eval_string(&lisp_code)?;
    
    Ok(())
}
```

**Primitives:**
- `(load-header-with-lib header-path lib-path)` → lib-handle
- `(ffi-call lib-name function-name arg-types return-type ...args)` → result
- `(define-enum name variants)` → enum-id
- `(define-c-struct name fields)` → struct-id

**Usage Example:**

```lisp
(load-header-with-lib "gtk/gtk.h" "/usr/lib/libgtk-4.so")

; Now all GTK4 functions are available as Elle functions
(let window (gtk-window-new GTK_WINDOW_TOPLEVEL))
(gtk-window-set-title window "Hello from Elle!")
(gtk-widget-show window)
```

**Tests:**
- Parse GTK4 headers
- Parse SDL2 headers
- Parse LLVM headers
- Generate valid Elle bindings
- Call generated functions
- Verify return values
- Test enum conversion
- Test struct marshaling

### Dependencies

```toml
bindgen = "0.69"          # C header parsing
```

### Implementation Notes

- Bindgen runs at runtime (not compile-time) to support dynamic header paths
- Cache parsed headers to avoid re-parsing
- Handle #define macros → Elle constants
- Support function pointer types for callbacks (Phase 4)
- Support variadic functions (printf, etc.) with type hints

---

## Phase 4: Advanced Features (Weeks 11-14)

### Goals
- C callbacks (call Elle functions from C)
- Memory management integration
- WASM support
- Error handling and safety

### Deliverables

#### 4.1 Callbacks (`src/ffi/callback.rs`)

C libraries often call back to application functions:

```rust
pub struct CallbackWrapper {
    closure_id: u32,              // Reference to Elle closure
    arg_types: Vec<CType>,
    return_type: CType,
    extern_fn: extern "C" fn(...) -> ...,  // Platform-specific
}

impl VM {
    pub fn create_c_callback(
        &mut self,
        closure: Value,
        arg_types: Vec<CType>,
        return_type: CType,
    ) -> Result<*const c_void, String> {
        // Create wrapper function
        // Store closure in VM callback table
        // Return pointer to wrapper
    }
}
```

**Usage in Elle:**

```lisp
; Define a callback function
(define (on-button-clicked widget data)
  (print "Button clicked!")
  nil)

; Register it with GTK
(let callback (make-c-callback on-button-clicked 
                 :args (:pointer :pointer)
                 :return :void))
(g-signal-connect! button "clicked" callback nil)
```

**Challenges:**
- Callbacks must be thread-safe
- Handle Elle evaluation from C context
- Proper error propagation back to C
- Memory safety (callback outlives Elle function?)

**Platform-Specific:**
- Linux: Trampoline functions + closure registry
- WASM: js-sys for browser callbacks or simulated via table

#### 4.2 Memory Management (`src/ffi/memory.rs`)

Track C allocations:

```rust
pub struct CMemoryManager {
    allocations: HashMap<*const c_void, AllocationInfo>,
}

pub struct AllocationInfo {
    type_id: TypeId,
    type_name: String,
    size: usize,
    owner: MemoryOwner,  // Elle or C
    free_fn: Option<fn(*const c_void)>,
}

pub enum MemoryOwner {
    Elle,  // Elle owns, can free when GC'd
    C,     // C owns, Elle holds reference
    Shared, // Both may own, coordinate
}

impl Drop for CValue {
    fn drop(&mut self) {
        if let MemoryOwner::Elle = self.memory_manager.get(self.ptr) {
            // Call appropriate free function
        }
    }
}
```

**Strategies:**

1. **Elle-Owned:** Elle allocates via C, frees via Drop
   - Use case: Temporary struct marshaling
   - `let s = (make-struct GtkAllocation ...)`

2. **C-Owned:** C allocates, Elle holds reference
   - Use case: GTK widgets created by gtk_window_new
   - Elle never frees, holds reference only
   - Crashes if C frees while Elle still holds ref (unsafe)

3. **Shared:** Explicit reference counting
   - Use case: Complex C objects with intricate ownership
   - Use wrapper types with manual refcount
   - Future: GObject-style reference counting

**Memory Safety Approach:**
- Track allocation source (C or Elle)
- Prevent double-free (CValue guards pointer)
- Use Option<CValue> for optional pointers
- Runtime checks in debug mode

#### 4.3 WASM Support (`src/ffi/wasm.rs`)

WASM introduces unique constraints:

```rust
#[cfg(target_arch = "wasm32")]
mod wasm_support {
    use wasm_bindgen::prelude::*;
    
    // WASM can't load .so files; instead:
    // 1. Pre-compile C code to WASM with emscripten
    // 2. Link into WASM binary
    // 3. Call via js-sys for browser APIs
    
    pub struct WasmFunctionHandle {
        name: String,
        func: js_sys::Function,  // JavaScript function
    }
    
    impl WasmFunctionHandle {
        pub fn call(&self, args: &[Value]) -> Result<Value, String> {
            // Marshal Elle values to JS types
            // Call JS function via wasm_bindgen
            // Marshal JS result back to Elle
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_support {
    // Linux native implementation using libloading, libffi
}
```

**WASM FFI Strategy:**

1. **Pre-compiled WASM Libraries:** C libraries compiled to WASM with Emscripten
   ```bash
   # Compile GTK to WASM (hypothetically)
   emcripten gtk-4.c -o gtk-4.wasm
   ```

2. **Linking:** Link into Elle binary at compile time
   ```rust
   // src/main.rs
   #[link(wasm_import_module = "gtk")]
   extern "C" {
       pub fn gtk_init(argc: *const i32, argv: *const *const u8);
   }
   ```

3. **Browser APIs:** Call browser functions via js-sys
   ```lisp
   (load-library "browser" :js-module "window")
   (fetch "http://example.com/data" 
     :on-response (fn [response] (print response)))
   ```

4. **Fallback:** Use WASI (WebAssembly System Interface) for file I/O
   ```lisp
   (open-file "data.txt" :mode :read)
   ```

**WASM Constraints:**
- 4GB linear memory (but practical limit ~1GB)
- No threads (except SharedArrayBuffer)
- No direct OS system calls (go through WASI or js-sys)
- 32-bit pointers (WASM32)
- No dlopen/dlsym

**Implementation:**
- Detect target architecture at runtime
- Use feature flags for platform-specific code
- Provide stub implementations on unsupported platforms

#### 4.4 Error Handling and Safety (`src/ffi/safety.rs`)

```rust
pub struct FFIError {
    pub kind: FFIErrorKind,
    pub message: String,
    pub location: SourceLoc,
}

pub enum FFIErrorKind {
    TypeMismatch { expected: CType, got: Value },
    SymbolNotFound { symbol: String, lib: String },
    LibraryNotLoaded { path: String },
    MarshalingFailed { direction: &'static str, reason: String },
    NullPointerDeref { type_name: String },
    SegmentationFault { address: *const c_void },
    OutOfMemory,
    InvalidStructLayout { struct_name: String },
}

pub fn safe_c_call<F>(f: F) -> Result<Value, FFIError>
where
    F: FnOnce() -> Result<Value, String>
{
    // Install signal handlers to catch SIGSEGV
    // Call function with safety wrapper
    // Restore signal handlers
    // Return error if segfault occurred
}
```

**Safety Features:**

1. **Type Checking** (compile-time in generated code)
   - Type signature of function is known
   - Arguments validated before marshaling
   - Return type validated after call

2. **Null Pointer Checks**
   - Detect null pointers from C
   - Return Option<CValue> in Elle
   - Automatic None → nil conversion

3. **Segmentation Fault Handling** (Linux)
   - Register SIGSEGV handler
   - Catch stack unwinding
   - Return error instead of crashing

4. **Memory Bounds Checking**
   - When marshaling arrays, check bounds
   - Prevent out-of-bounds access

5. **Timeout Support** (future)
   - Prevent infinite loops in C code
   - Use alarm() or thread-based timeout

**Primitives:**
- `(with-ffi-safety-checks body)` → result
- `(null? c-value)` → bool
- `(ffi-last-error)` → error-message

**Tests:**
- Call functions and verify results
- Handle null pointers gracefully
- Test type checking
- Verify error messages

### Dependencies

```toml
wasm-bindgen = "0.2"      # WASM interop
js-sys = "0.3"            # JavaScript APIs
```

---

## Phase 5: GTK4, SDL2, LLVM Examples (Weeks 15-16)

### Goals
- Real-world examples using major libraries
- Demonstrate full feature set
- Create reusable binding templates

### Deliverables

#### 5.1 GTK4 Binding Example

**File: `examples/gtk4-app.lisp`**

```lisp
; Load GTK4 with auto-generated bindings
(load-header-with-lib "/usr/include/gtk-4/gtk.h" 
                       "/usr/lib/libgtk-4.so.1")

; Create application
(let app (gtk-application-new "com.example.HelloElleGTK" 0))

; Define activate callback
(define (on-activate app)
  (let window (gtk-application-window-new app))
  (gtk-window-set-title window "Hello from Elle!")
  (gtk-window-set-default-size window 400 300)
  
  (let box (gtk-box-new GTK_ORIENTATION_VERTICAL 0))
  (gtk-widget-set-margin-top box 12)
  (gtk-widget-set-margin-bottom box 12)
  (gtk-widget-set-margin-start box 12)
  (gtk-widget-set-margin-end box 12)
  
  (let label (gtk-label-new "Welcome to Elle!"))
  (gtk-box-append box label)
  
  (let button (gtk-button-new-with-label "Click me"))
  (g-signal-connect! button "clicked" 
    (make-c-callback 
      (fn [widget data]
        (print "Button clicked from Elle!")
        (gtk-window-close window))
      :args (:pointer :pointer)
      :return :void)
    nil)
  (gtk-box-append box button)
  
  (gtk-window-set-child window box)
  (gtk-widget-show window))

; Connect and run
(g-signal-connect! app "activate" 
  (make-c-callback on-activate :args (:pointer :pointer) :return :void) 
  nil)

(g-application-run app 0 nil)
```

**Output:** GTK4 window with label and button, callback handling

#### 5.2 SDL2 Binding Example

**File: `examples/sdl2-game.lisp`**

```lisp
(load-header-with-lib "/usr/include/SDL2/SDL.h" 
                       "/usr/lib/libSDL2.so")

; Initialize SDL
(sdl-init SDL_INIT_VIDEO)

; Create window
(let window (sdl-create-window "Elle SDL2 Game" 
                               SDL_WINDOWPOS_CENTERED
                               SDL_WINDOWPOS_CENTERED
                               800 600
                               0))

; Create renderer
(let renderer (sdl-create-renderer window -1 0))

; Game loop
(let running #t)
(while running
  (let event (make-struct SDL_Event))
  (if (sdl-poll-event! event)
    (match (struct-get event 'type)
      (SDL_QUIT (set! running #f))
      (_ nil)))
  
  ; Clear and draw
  (sdl-set-render-draw-color renderer 0 0 0 255)
  (sdl-render-clear renderer)
  
  ; Draw rectangle
  (let rect (make-struct SDL_Rect))
  (struct-set! rect 'x 100)
  (struct-set! rect 'y 100)
  (struct-set! rect 'w 100)
  (struct-set! rect 'h 100)
  (sdl-set-render-draw-color renderer 255 0 0 255)
  (sdl-render-fill-rect renderer rect)
  
  (sdl-render-present renderer))

; Cleanup
(sdl-destroy-renderer renderer)
(sdl-destroy-window window)
(sdl-quit)
```

**Output:** SDL2 window with animated rectangle

#### 5.3 LLVM Binding Example

**File: `examples/llvm-compiler.lisp`**

```lisp
(load-header-with-lib "/usr/include/llvm-c/Core.h"
                       "/usr/lib/libLLVM.so")

; Create module
(let module (llvm-module-create-with-name "hello"))

; Create function type (i32 func())
(let i32-type (llvm-int32-type))
(let func-type (llvm-function-type i32-type #() #f))

; Add function to module
(let func (llvm-add-function module "main" func-type))

; Create basic block
(let context (llvm-get-global-context))
(let builder (llvm-create-builder-in-context context))
(let bb (llvm-append-basic-block-in-context context func "entry"))

; Build function body
(llvm-position-builder-at-end builder bb)
(let forty-two (llvm-const-int i32-type 42 #f))
(llvm-build-ret builder forty-two)

; Dump module
(llvm-dump-module module)

; Cleanup
(llvm-dispose-builder builder)
(llvm-dispose-module module)
```

**Output:** LLVM IR dump showing generated module

#### 5.4 Binding Templates

**File: `templates/binding-generator.lisp`**

```lisp
; Meta-tool to generate bindings from headers

(define (generate-binding header-path lib-path output-path)
  (let parsed (parse-c-header header-path))
  
  (let file (open-file output-path :mode :write))
  
  ; Generate header comment
  (fprintf file "; Auto-generated from ~a~n" header-path)
  (fprintf file "; Library: ~a~n~n" lib-path)
  
  ; Generate type definitions
  (for-each (fn [struct-def]
    (fprintf file "(define-c-struct ~a ~{~a~})~n"
      (struct-name struct-def)
      (struct-fields struct-def)))
    (parsed-structs parsed))
  
  ; Generate function wrappers
  (for-each (fn [func-def]
    (generate-function-wrapper file func-def lib-path))
    (parsed-functions parsed))
  
  (close-file file)
  (printf "Generated ~a~n" output-path))
```

### Tests

- Compile and run GTK4 example
- Compile and run SDL2 example
- Compile and run LLVM example
- Verify window creation (GTK4)
- Verify graphics rendering (SDL2)
- Verify LLVM IR generation

---

## Implementation Timeline

| Phase | Duration | Features | Risk |
|-------|----------|----------|------|
| 1: Core FFI | 3 weeks | Dynamic loading, basic types, libffi | Low |
| 2: Marshaling | 3 weeks | Structs, arrays, nested types | Medium |
| 3: Header Parsing | 4 weeks | Bindgen integration, auto-generation | Medium |
| 4: Advanced | 4 weeks | Callbacks, memory mgmt, WASM, safety | High |
| 5: Examples | 2 weeks | GTK4, SDL2, LLVM demos | Low |
| **Total** | **16 weeks** | **Full FFI system** | **Medium** |

---

## Risk Analysis

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| Bindgen complexity | Medium | High | Use stable bindgen 0.69, limit feature set |
| WASM limitations | Medium | Medium | Plan dual code paths early, test frequently |
| Type system gaps | Low | High | Comprehensive type tests in Phase 2 |
| C calling convention bugs | Low | High | Use libffi crate (battle-tested) |
| Memory safety violations | Medium | Critical | Strict ownership tracking, safety tests |
| Performance overhead | Low | Medium | Benchmark Phase 1, optimize marshaling |
| Cross-platform differences | Medium | Medium | Test on Linux first, WASM later |

---

## Success Criteria

### Phase 1 ✓
- [ ] Load .so files dynamically on Linux
- [ ] Call strlen, abs, sin with correct results
- [ ] Handle symbol resolution failures gracefully
- [ ] 15+ unit tests passing
- [ ] Call overhead < 1µs (libffi overhead)

### Phase 2 ✓
- [ ] Define and use custom structs
- [ ] Pass structs to C functions
- [ ] Receive struct results from C
- [ ] Array marshaling working
- [ ] 25+ tests passing

### Phase 3 ✓
- [ ] Parse GTK4 headers
- [ ] Auto-generate Elle bindings
- [ ] Call generated GTK4 functions
- [ ] Parse SDL2 headers
- [ ] Parse LLVM headers
- [ ] 20+ parsing tests

### Phase 4 ✓
- [ ] C callbacks work (signal handlers)
- [ ] Memory ownership tracked correctly
- [ ] WASM stubs implemented
- [ ] Segfault safety checks working
- [ ] 30+ integration tests

### Phase 5 ✓
- [ ] GTK4 window creation example works
- [ ] SDL2 rendering example works
- [ ] LLVM IR generation example works
- [ ] All examples include proper error handling
- [ ] Documentation complete

---

## Architecture Decisions

### Why libffi Instead of Manual Calling Convention?

**Pros of libffi:**
- Battle-tested calling convention handling (x86-64, ARM, etc.)
- Platform-independent API
- Handles edge cases we'd miss
- Active maintenance
- Smaller code footprint

**Cons:**
- Slight performance overhead (~100-500ns per call)
- One more dependency

**Decision:** Use libffi for correctness, optimize hot paths later if needed

### Why Bindgen Instead of Manual Header Parsing?

**Pros of bindgen:**
- Handles C preprocessor (#define, #ifdef, etc.)
- Supports complex types and forward declarations
- Actively maintained by Rust FFI community
- Works with system headers out of box
- Can handle some C++ (for LLVM headers)

**Cons:**
- Larger dependency
- May generate code we don't use (we'll filter)

**Decision:** Use bindgen for correctness, generate minimal Elle code

### Why Two Code Paths (Linux vs WASM)?

**Alternatives:**
1. Native-only (simpler, but no WASM support) ✗
2. WASM-only (no native performance) ✗
3. Dual path (complex but maximum compatibility) ✓

**Decision:** Feature-gate platform code, test both paths regularly

### Why CHandle vs Embedding in Value?

**Question:** Should opaque C pointers be a Value variant or wrapped differently?

**Options:**
1. `Value::CHandle(CValue)` - Direct variant ✓
2. `Value::NativeFn` reuse - Confusing semantics ✗
3. Separate registry - Lookup overhead ✗

**Decision:** CHandle variant with attached type information

---

## Testing Strategy

### Unit Tests (src/ffi/*.rs)
- Type size calculations
- Marshaling round-trips
- Struct layout computation
- Symbol resolution
- Error conditions

### Integration Tests (tests/ffi_*.rs)
- Load system libraries (libc, libm)
- Call standard functions
- GTK4 window creation
- SDL2 rendering
- LLVM compilation
- WASM module loading (on WASM target)

### Example Tests
- Run gtk4-app.lisp and verify window appears
- Run sdl2-game.lisp and verify rendering
- Run llvm-compiler.lisp and verify IR output
- Benchmark FFI call overhead

### Platform Tests
- Linux x86-64 (primary)
- WASM32 (secondary, use wasmtime or browser)
- Check calling convention handling
- Verify struct alignment

---

## Performance Targets

| Operation | Target | Acceptable | Fail |
|-----------|--------|-----------|------|
| Load library | <10ms | <100ms | >1s |
| Parse header | <100ms | <1s | >10s |
| Generate bindings | <50ms | <200ms | >1s |
| Simple function call | <1µs | <10µs | >100µs |
| Struct marshaling | <10µs | <50µs | >500µs |
| Callback invocation | <10µs | <100µs | >1ms |

**Benchmarks to Add:**
```rust
#[bench]
fn bench_simple_c_call(b: &mut Bencher) {
    // Call strlen from libc
}

#[bench]
fn bench_struct_marshal(b: &mut Bencher) {
    // Create struct and pass to C
}

#[bench]
fn bench_callback_invoke(b: &mut Bencher) {
    // Invoke C callback → Elle function
}
```

---

## Documentation Plan

### User Documentation
1. **FFI Guide** - How to use FFI in Elle
2. **Binding GTK4** - Step-by-step GTK4 integration
3. **Binding SDL2** - Step-by-step SDL2 integration
4. **Binding LLVM** - Step-by-step LLVM integration
5. **Safety & Performance** - Best practices

### Developer Documentation
1. **FFI Architecture** - Internal design details
2. **Adding New Types** - How to extend type system
3. **Platform-Specific Code** - Linux vs WASM paths
4. **Testing Guide** - How to test FFI features

### Code Examples
- Hello World (simple function call)
- GTK4 window with callbacks
- SDL2 game loop
- LLVM IR generation
- Custom type marshaling

---

## Future Extensions (Post-MVP)

### 10. Async/Await FFI
- Call async C functions (wait-free patterns)
- Return Promises/futures from C
- Integrate with Elle async runtime

### 11. Macro-Based Binding Generation
```lisp
(defmacro c-fun ((name lib-name) return-type &rest args)
  `(define (,name . args)
     (ffi-call ,lib-name ,(symbol->string name) 
       ,args ,return-type)))

(c-fun (strlen libc) :int :pointer)
```

### 12. Inline C Expressions
```lisp
(c-inline 
  "int result = x + y;"
  ((x :int) (y :int))
  :int)
```

### 13. GObject Introspection Support
- Use GObject's runtime type information
- Auto-generate bindings for any GObject library
- Handle reference counting automatically

### 14. JIT-compiled FFI
- Compile hot FFI call paths to native code
- Bypass Elle interpreter overhead
- Cache compiled trampolines

---

## References and Resources

### C FFI Standards
- System V AMD64 ABI: https://refspecs.linuxbase.org/elf/x86-64-abi-0.99.pdf
- WASI (WebAssembly System Interface): https://wasi.dev
- Emscripten C Runtime: https://emscripten.org

### Key Dependencies
- libffi: https://github.com/libffi/libffi
- bindgen: https://github.com/rust-lang/rust-bindgen
- libloading: https://github.com/nagisa/rust_libloading
- wasm-bindgen: https://github.com/rustwasm/wasm-bindgen

### Library Documentation
- GTK4: https://docs.gtk.org/gtk4/
- SDL2: https://wiki.libsdl.org/SDL2/
- LLVM C API: https://llvm.org/doxygen/

### Related Tools
- pkg-config (find system libraries): https://www.freedesktop.org/wiki/Software/pkg_config/
- clang (parse C headers): https://clang.llvm.org
- Emscripten (WASM compilation): https://emscripten.org

---

## Appendix: Example Implementation Stub

### src/ffi/mod.rs

```rust
pub mod loader;
pub mod symbol;
pub mod types;
pub mod marshal;
pub mod call;
pub mod header;
pub mod bindings;
pub mod callback;
pub mod memory;
pub mod safety;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

use crate::value::Value;
use std::collections::HashMap;

pub struct FFISubsystem {
    libraries: HashMap<u32, LibraryHandle>,
    structs: HashMap<String, StructLayout>,
    functions: HashMap<String, FunctionHandle>,
    callbacks: HashMap<u32, CallbackHandle>,
}

impl FFISubsystem {
    pub fn new() -> Self {
        FFISubsystem {
            libraries: HashMap::new(),
            structs: HashMap::new(),
            functions: HashMap::new(),
            callbacks: HashMap::new(),
        }
    }
    
    pub fn load_library(&mut self, path: &str) -> Result<u32, String> {
        #[cfg(target_arch = "wasm32")]
        return self.wasm.load_library(path);
        
        #[cfg(not(target_arch = "wasm32"))]
        return self.native.load_library(path);
    }
}
```

### Example Primitive: load-library

```rust
fn prim_load_library(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("load-library requires at least 1 argument".to_string());
    }
    
    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("load-library requires string path".to_string()),
    };
    
    // Handle optional :header argument
    let _header_path = if args.len() > 1 {
        match &args[1] {
            Value::String(s) => Some(s.as_ref()),
            _ => None,
        }
    } else {
        None
    };
    
    // TODO: Call ffi subsystem to load library
    // let lib_handle = vm.ffi.load_library(path)?;
    
    Ok(Value::LibraryHandle(LibHandle { id: 0 }))
}
```

---

## Conclusion

This FFI roadmap provides:

1. **Clear path to production** - 5 phases, 16 weeks, well-defined deliverables
2. **Support for major libraries** - GTK4, SDL2, LLVM explicitly in scope
3. **Cross-platform design** - Linux native + WASM support planned from start
4. **Type safety** - Leverages C headers via bindgen for correctness
5. **Performance-conscious** - Uses libffi for efficiency, benchmarks included
6. **Testing strategy** - Unit, integration, and example-based validation
7. **Safety first** - Proper memory tracking, error handling, segfault safety
8. **Future-proof** - Extensible design supports callbacks, async, macros

Elle will evolve from a pure Lisp interpreter to a **Lisp-first systems programming language** capable of interfacing with the entire C/WASM ecosystem.
