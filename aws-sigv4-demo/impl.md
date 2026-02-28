# Implementation Instructions: Issue #372 — Remaining Work

All sections are mechanically specified. Each section gets its own commit.
Verification after each: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt -- --check`

---

## Section 1: OOB Fix (already done, needs commit)

### Status: Working tree contains these changes

**File**: `src/primitives/table.rs` — `prim_get` bytes/blob branches return
`(SIG_OK, default)` on out-of-bounds index (unchanged from current code — the
OOB behavior already returns `default`, not `SIG_ERROR`).

**File**: `tests/integration/bytes.rs` — contains OOB tests (verify they exist
at end of file; if not, add):

```rust
#[test]
fn test_bytes_get_oob_returns_default() {
    let result = eval_source("(get (bytes 1 2 3) 10 :missing)").unwrap();
    assert_eq!(result.as_keyword_name(), Some("missing"));
}

#[test]
fn test_blob_get_oob_returns_default() {
    let result = eval_source("(get (blob 1 2 3) 10 :missing)").unwrap();
    assert_eq!(result.as_keyword_name(), Some("missing"));
}
```

**Action**: Commit these changes as-is. No code changes needed.

---

## Section 2: Remaining Part 3 Primitives

**Note**: `uri-encode` is already implemented in `src/primitives/string.rs`.

### 2a. New functions in `src/primitives/bytes.rs`

Add these functions after `prim_bytes_to_blob`:

```rust
/// Append a byte (int 0-255) to a blob. Returns the blob.
pub fn prim_blob_push(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (SIG_ERROR, error_val("arity-error",
            format!("blob/push: expected 2 arguments, got {}", args.len())));
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/push: expected blob, got {}", args[0].type_name()))),
    };
    let byte = match args[1].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => return (SIG_ERROR, error_val("error",
            format!("blob/push: byte out of range 0-255: {}", n))),
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/push: expected integer, got {}", args[1].type_name()))),
    };
    blob_ref.borrow_mut().push(byte);
    (SIG_OK, args[0])
}

/// Remove and return last byte from blob as int. Error on empty.
pub fn prim_blob_pop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_ERROR, error_val("arity-error",
            format!("blob/pop: expected 1 argument, got {}", args.len())));
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/pop: expected blob, got {}", args[0].type_name()))),
    };
    match blob_ref.borrow_mut().pop() {
        Some(byte) => (SIG_OK, Value::int(byte as i64)),
        None => (SIG_ERROR, error_val("error", "blob/pop: empty blob".to_string())),
    }
}

/// Set byte at index in blob. Error on OOB.
pub fn prim_blob_put(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (SIG_ERROR, error_val("arity-error",
            format!("blob/put: expected 3 arguments, got {}", args.len())));
    }
    let blob_ref = match args[0].as_blob() {
        Some(b) => b,
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/put: expected blob, got {}", args[0].type_name()))),
    };
    let index = match args[1].as_int() {
        Some(i) => i,
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/put: index must be integer, got {}", args[1].type_name()))),
    };
    let byte = match args[2].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => return (SIG_ERROR, error_val("error",
            format!("blob/put: byte out of range 0-255: {}", n))),
        None => return (SIG_ERROR, error_val("type-error",
            format!("blob/put: expected integer, got {}", args[2].type_name()))),
    };
    let len = blob_ref.borrow().len();
    if index < 0 || (index as usize) >= len {
        return (SIG_ERROR, error_val("error",
            format!("blob/put: index {} out of bounds (length {})", index, len)));
    }
    blob_ref.borrow_mut()[index as usize] = byte;
    (SIG_OK, args[0])
}

/// Slice a bytes or blob. Returns same type as input.
/// (slice coll start end)
pub fn prim_slice(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (SIG_ERROR, error_val("arity-error",
            format!("slice: expected 3 arguments, got {}", args.len())));
    }
    let start = match args[1].as_int() {
        Some(i) if i >= 0 => i as usize,
        Some(i) => return (SIG_ERROR, error_val("error",
            format!("slice: start must be non-negative, got {}", i))),
        None => return (SIG_ERROR, error_val("type-error",
            format!("slice: start must be integer, got {}", args[1].type_name()))),
    };
    let end = match args[2].as_int() {
        Some(i) if i >= 0 => i as usize,
        Some(i) => return (SIG_ERROR, error_val("error",
            format!("slice: end must be non-negative, got {}", i))),
        None => return (SIG_ERROR, error_val("type-error",
            format!("slice: end must be integer, got {}", args[2].type_name()))),
    };
    if let Some(b) = args[0].as_bytes() {
        let clamped_start = start.min(b.len());
        let clamped_end = end.min(b.len());
        if clamped_start > clamped_end {
            return (SIG_OK, Value::bytes(vec![]));
        }
        return (SIG_OK, Value::bytes(b[clamped_start..clamped_end].to_vec()));
    }
    if let Some(blob_ref) = args[0].as_blob() {
        let borrowed = blob_ref.borrow();
        let clamped_start = start.min(borrowed.len());
        let clamped_end = end.min(borrowed.len());
        if clamped_start > clamped_end {
            return (SIG_OK, Value::blob(vec![]));
        }
        return (SIG_OK, Value::blob(borrowed[clamped_start..clamped_end].to_vec()));
    }
    (SIG_ERROR, error_val("type-error",
        format!("slice: expected bytes or blob, got {}", args[0].type_name())))
}

/// buffer->bytes: convert buffer to immutable bytes
pub fn prim_buffer_to_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_ERROR, error_val("arity-error",
            format!("buffer->bytes: expected 1 argument, got {}", args.len())));
    }
    match args[0].as_buffer() {
        Some(buf_ref) => (SIG_OK, Value::bytes(buf_ref.borrow().clone())),
        None => (SIG_ERROR, error_val("type-error",
            format!("buffer->bytes: expected buffer, got {}", args[0].type_name()))),
    }
}

/// buffer->blob: convert buffer to mutable blob
pub fn prim_buffer_to_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_ERROR, error_val("arity-error",
            format!("buffer->blob: expected 1 argument, got {}", args.len())));
    }
    match args[0].as_buffer() {
        Some(buf_ref) => (SIG_OK, Value::blob(buf_ref.borrow().clone())),
        None => (SIG_ERROR, error_val("type-error",
            format!("buffer->blob: expected buffer, got {}", args[0].type_name()))),
    }
}

/// bytes->buffer: convert bytes to buffer. Error on invalid UTF-8.
pub fn prim_bytes_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_ERROR, error_val("arity-error",
            format!("bytes->buffer: expected 1 argument, got {}", args.len())));
    }
    match args[0].as_bytes() {
        Some(b) => match std::str::from_utf8(b) {
            Ok(_) => (SIG_OK, Value::buffer(b.to_vec())),
            Err(e) => (SIG_ERROR, error_val("error",
                format!("bytes->buffer: invalid UTF-8: {}", e))),
        },
        None => (SIG_ERROR, error_val("type-error",
            format!("bytes->buffer: expected bytes, got {}", args[0].type_name()))),
    }
}

/// blob->buffer: convert blob to buffer. Error on invalid UTF-8.
pub fn prim_blob_to_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_ERROR, error_val("arity-error",
            format!("blob->buffer: expected 1 argument, got {}", args.len())));
    }
    match args[0].as_blob() {
        Some(blob_ref) => {
            let borrowed = blob_ref.borrow();
            match std::str::from_utf8(&borrowed) {
                Ok(_) => (SIG_OK, Value::buffer(borrowed.clone())),
                Err(e) => (SIG_ERROR, error_val("error",
                    format!("blob->buffer: invalid UTF-8: {}", e))),
            }
        }
        None => (SIG_ERROR, error_val("type-error",
            format!("blob->buffer: expected blob, got {}", args[0].type_name()))),
    }
}
```

Add to the `PRIMITIVES` array in `bytes.rs` (after the `blob->hex` entry):

```rust
PrimitiveDef {
    name: "slice",
    func: prim_slice,
    effect: Effect::none(),
    arity: Arity::Exact(3),
    doc: "Slice bytes or blob from start to end index.",
    params: &["coll", "start", "end"],
    category: "bytes",
    example: "(slice (bytes 1 2 3 4 5) 1 3)",
    aliases: &[],
},
PrimitiveDef {
    name: "buffer->bytes",
    func: prim_buffer_to_bytes,
    effect: Effect::none(),
    arity: Arity::Exact(1),
    doc: "Convert buffer to immutable bytes.",
    params: &["buf"],
    category: "bytes",
    example: "(buffer->bytes @\"hello\")",
    aliases: &[],
},
PrimitiveDef {
    name: "buffer->blob",
    func: prim_buffer_to_blob,
    effect: Effect::none(),
    arity: Arity::Exact(1),
    doc: "Convert buffer to mutable blob.",
    params: &["buf"],
    category: "bytes",
    example: "(buffer->blob @\"hello\")",
    aliases: &[],
},
PrimitiveDef {
    name: "bytes->buffer",
    func: prim_bytes_to_buffer,
    effect: Effect::none(),
    arity: Arity::Exact(1),
    doc: "Convert bytes to buffer. Errors on invalid UTF-8.",
    params: &["b"],
    category: "bytes",
    example: "(bytes->buffer (bytes 104 105))",
    aliases: &[],
},
PrimitiveDef {
    name: "blob->buffer",
    func: prim_blob_to_buffer,
    effect: Effect::none(),
    arity: Arity::Exact(1),
    doc: "Convert blob to buffer. Errors on invalid UTF-8.",
    params: &["b"],
    category: "bytes",
    example: "(blob->buffer (blob 104 105))",
    aliases: &[],
},
```

### 2b. Wire `push` on blob into `src/primitives/array.rs`

In `prim_push`, after the buffer branch (after line 101 `return (SIG_OK, args[0]);`),
add before the final error return:

```rust
if let Some(blob_ref) = args[0].as_blob() {
    let byte = match args[1].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val("error", format!("push: byte value out of range 0-255: {}", n)),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val("type-error",
                    format!("push: blob value must be integer, got {}", args[1].type_name())),
            )
        }
    };
    blob_ref.borrow_mut().push(byte);
    return (SIG_OK, args[0]);
}
```

Update the final error message to include "blob":
`"push: expected array, buffer, or blob, got {}"`.

### 2c. Wire `pop` on blob into `src/primitives/array.rs`

In `prim_pop`, after the buffer branch (after line 159 closing brace),
add before the final error return:

```rust
if let Some(blob_ref) = args[0].as_blob() {
    let mut blob = blob_ref.borrow_mut();
    match blob.pop() {
        Some(byte) => {
            drop(blob);
            return (SIG_OK, Value::int(byte as i64));
        }
        None => {
            drop(blob);
            return (SIG_ERROR, error_val("error", "pop: empty blob".to_string()));
        }
    }
}
```

Update the final error message to include "blob":
`"pop: expected array, buffer, or blob, got {}"`.

### 2d. Wire `put` on blob into `src/primitives/table.rs`

In `prim_put`, after the buffer branch (after line 745 `return (SIG_OK, args[0]);`),
add before the array branch:

```rust
// Blob (mutable byte sequence) - mutate in place
if let Some(blob_ref) = args[0].as_blob() {
    let index = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error",
                    format!("put: blob index must be integer, got {}", args[1].type_name())),
            )
        }
    };
    let byte = match args[2].as_int() {
        Some(n) if (0..=255).contains(&n) => n as u8,
        Some(n) => {
            return (
                SIG_ERROR,
                error_val("error", format!("put: byte value out of range 0-255: {}", n)),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val("type-error",
                    format!("put: blob value must be integer, got {}", args[2].type_name())),
            )
        }
    };
    let len = blob_ref.borrow().len();
    if index < 0 || (index as usize) >= len {
        return (
            SIG_ERROR,
            error_val("error", format!("put: index {} out of bounds (length {})", index, len)),
        );
    }
    blob_ref.borrow_mut()[index as usize] = byte;
    return (SIG_OK, args[0]);
}
```

### 2e. Wire `append` on bytes/blob into `src/primitives/list.rs`

In `prim_append`, after the string branch (after line 435 closing brace),
add before the list branch:

```rust
// Bytes (immutable) - return new bytes
if let Some(b) = args[0].as_bytes() {
    if let Some(other_b) = args[1].as_bytes() {
        let mut result = b.to_vec();
        result.extend(other_b);
        return (SIG_OK, Value::bytes(result));
    } else {
        return (
            SIG_ERROR,
            error_val("type-error",
                format!("append: both arguments must be same type, got bytes and {}",
                    args[1].type_name())),
        );
    }
}

// Blob (mutable) - mutate in place
if let Some(blob_ref) = args[0].as_blob() {
    if let Some(other_blob_ref) = args[1].as_blob() {
        let other_borrowed = other_blob_ref.borrow();
        let mut borrowed = blob_ref.borrow_mut();
        borrowed.extend(other_borrowed.iter());
        drop(borrowed);
        return (SIG_OK, args[0]);
    } else {
        return (
            SIG_ERROR,
            error_val("type-error",
                format!("append: both arguments must be same type, got blob and {}",
                    args[1].type_name())),
        );
    }
}
```

### 2f. Registration

`src/primitives/bytes.rs` PRIMITIVES array already includes the existing
entries. The new entries from 2a are added to the same array. No changes
needed in `registration.rs` — the bytes module is already registered.

The `blob/push`, `blob/pop`, `blob/put` functions are NOT registered as
separate primitives — they are standalone functions in `bytes.rs` that
exist only for direct Rust-level use. The polymorphic `push`/`pop`/`put`
in `array.rs`/`table.rs` dispatch to blob via the branches added in 2b-2d.

### 2g. Tests

Add to `tests/integration/bytes.rs`:

```rust
#[test]
fn test_blob_push() {
    let result = eval_source("(let ((b (blob 1 2))) (push b 3) b)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[1, 2, 3]);
}

#[test]
fn test_blob_pop() {
    let result = eval_source("(let ((b (blob 1 2 3))) (pop b))").unwrap();
    assert_eq!(result.as_int().unwrap(), 3);
}

#[test]
fn test_blob_put() {
    let result = eval_source("(let ((b (blob 1 2 3))) (put b 1 99) (get b 1))").unwrap();
    assert_eq!(result.as_int().unwrap(), 99);
}

#[test]
fn test_slice_bytes() {
    let result = eval_source("(slice (bytes 1 2 3 4 5) 1 3)").unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), &[2, 3]);
}

#[test]
fn test_slice_blob() {
    let result = eval_source("(slice (blob 1 2 3 4 5) 1 3)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[2, 3]);
}

#[test]
fn test_append_bytes() {
    let result = eval_source("(append (bytes 1 2) (bytes 3 4))").unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), &[1, 2, 3, 4]);
}

#[test]
fn test_append_blob() {
    let result = eval_source("(let ((a (blob 1 2)) (b (blob 3 4))) (append a b) a)").unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], &[1, 2, 3, 4]);
}

#[test]
fn test_buffer_to_bytes() {
    let result = eval_source(r#"(buffer->bytes @"hello")"#).unwrap();
    assert!(result.is_bytes());
    assert_eq!(result.as_bytes().unwrap(), b"hello");
}

#[test]
fn test_buffer_to_blob() {
    let result = eval_source(r#"(buffer->blob @"hello")"#).unwrap();
    assert!(result.is_blob());
    assert_eq!(&result.as_blob().unwrap().borrow()[..], b"hello");
}

#[test]
fn test_bytes_to_buffer() {
    let result = eval_source("(buffer->string (bytes->buffer (bytes 104 105)))").unwrap();
    assert_eq!(result.as_string().unwrap(), "hi");
}

#[test]
fn test_blob_to_buffer() {
    let result = eval_source("(buffer->string (blob->buffer (blob 104 105)))").unwrap();
    assert_eq!(result.as_string().unwrap(), "hi");
}
```

**Note**: `test_bytes_to_buffer` and `test_blob_to_buffer` call
`as_string()` on short strings ("hi" — 2 bytes, SSO-eligible). These will
need migration to `with_string()` when SSO lands in Section 7.

---

## Section 3: Polymorphic keys/values/del

### Status: Already done — no-op

Verified by reading `src/primitives/table.rs`:

- `prim_keys` (line 481): dispatches on `is_table()` and `is_struct()`. ✓
- `prim_values` (line 559): dispatches on `is_table()` and `is_struct()`. ✓
- `prim_del` (line 145): dispatches on `is_table()` and `is_struct()`. ✓

No separate `struct/del` primitive exists. No changes needed.

---

## Section 4: `each` as Prelude Macro

### 4a. Remove `HirKind::For` variant

**File**: `src/hir/expr.rs`

Delete lines 145-149:

```rust
    /// For/each loop
    For {
        var: Binding,
        iter: Box<Hir>,
        body: Box<Hir>,
    },
```

### 4b. Remove `analyze_for` and the `"each"` match arm

**File**: `src/hir/analyze/forms.rs`

Delete line 163:
```rust
                        "each" => return self.analyze_for(items, span),
```

Delete the entire `analyze_for` method (lines 437-482).

### 4c. Remove `lower_for` and the `HirKind::For` match arm

**File**: `src/lir/lower/expr.rs`

Delete line 62:
```rust
            HirKind::For { var, iter, body } => self.lower_for(*var, iter, body),
```

Delete the entire `lower_for` method (starts at line 343, runs ~130 lines —
find the closing brace by matching the `fn lower_for` signature).

### 4d. Remove `HirKind::For` from tailcall.rs

**File**: `src/hir/tailcall.rs`

Delete lines 146-150 (the `mark` function's `For` arm):
```rust
        // For: loop bodies are never in tail position
        HirKind::For { iter, body, .. } => {
            mark(iter, false);
            mark(body, false);
        }
```

Delete lines 289-292 (the `collect_calls` function's `For` arm):
```rust
            HirKind::For { iter, body, .. } => {
                collect_calls(iter, calls);
                collect_calls(body, calls);
            }
```

### 4e. Remove `HirKind::For` from lint.rs

**File**: `src/hir/lint.rs`

Delete lines 166-169:
```rust
            HirKind::For { iter, body, .. } => {
                self.check(iter, symbols);
                self.check(body, symbols);
            }
```

### 4f. Remove `HirKind::For` from symbols.rs

**File**: `src/hir/symbols.rs`

Delete lines 203-207:
```rust
            HirKind::For { var, iter, body } => {
                self.record_definition(*var, SymbolKind::Variable, &hir.span, index, symbols);
                self.walk(iter, index, symbols);
                self.walk(body, index, symbols);
            }
```

### 4g. Add `each` macro to prelude

**File**: `prelude.lisp` (at repo root)

The prelude uses `defmacro` with `& <name>` for rest params, backtick for
quasiquote, `,` for unquote, `,@` for splice. `gensym` produces unique
symbols. `let*` is already a prelude macro. `while` is a special form.

Add at the end of the file:

```lisp
;; each - iterate over a sequence
;; Dispatches on type: lists use first/rest, indexed types use get/length,
;; strings use char-at/length.
;; (each x coll body...) or (each x in coll body...)
(defmacro each (var iter-or-in & forms)
  (let* ((has-in (and (not (empty? forms))
                      (not (empty? (rest forms)))
                      (= (syntax->datum iter-or-in) 'in)))
         (iter (if has-in (first forms) iter-or-in))
         (body (if has-in (rest forms) forms))
         (g-iter (gensym))
         (g-idx (gensym))
         (g-len (gensym))
         (g-cur (gensym)))
    `(let ((,g-iter ,iter))
       (cond
         ((pair? ,g-iter)
          (let* ((,g-cur ,g-iter))
            (while (pair? ,g-cur)
              (begin
                (let ((,var (first ,g-cur)))
                  ,@body)
                (set ,g-cur (rest ,g-cur))))))
         ((or (array? ,g-iter) (tuple? ,g-iter) (bytes? ,g-iter) (blob? ,g-iter))
          (let* ((,g-len (length ,g-iter))
                 (,g-idx 0))
            (while (< ,g-idx ,g-len)
              (begin
                (let ((,var (get ,g-iter ,g-idx)))
                  ,@body)
                (set ,g-idx (+ ,g-idx 1))))))
         ((or (string? ,g-iter) (buffer? ,g-iter))
          (let* ((,g-len (length ,g-iter))
                 (,g-idx 0))
            (while (< ,g-idx ,g-len)
              (begin
                (let ((,var (string/char-at ,g-iter ,g-idx)))
                  ,@body)
                (set ,g-idx (+ ,g-idx 1))))))
         (true (error :type-error "each: not a sequence")))))
```

**Note**: `while` takes exactly one body expression (`(while cond body)` —
see `analyze_while` in `src/hir/analyze/forms.rs` line 396). Each branch
generates two expressions (the inner `let` + the `set`), so they must be
wrapped in `begin`.

**Note**: Check the actual name of the char-at primitive. Look at
`src/primitives/string.rs` PRIMITIVES array for the registered name.
It may be `string/char-at` or `char-at`. Use whatever is registered.
If it's `string/char-at`, use that. If there's an alias `char-at`, either
works.

**Note**: The `(each x in coll body...)` syntax with optional `in` keyword
must be preserved for backward compatibility. The macro handles this by
checking if the second argument is the symbol `in`.

### 4h. Verify no `lir/emit.rs` changes needed

The emitter (`src/lir/emit.rs`) has no `For`-specific emission code. The
`lower_for` method in `lower/expr.rs` produced generic LIR instructions
(LoadLocal, StoreLocal, IsPair, Car, Cdr, Branch, Jump). Removing
`lower_for` is sufficient.

### 4i. Tests

Existing tests that use `each` should continue to pass since the macro
produces equivalent behavior. Run the full test suite. If any test
references `HirKind::For` directly (unlikely — check with grep), update it.

Add a test to `tests/integration/bytes.rs`:

```rust
#[test]
fn test_each_over_bytes() {
    let result = eval_source(r#"
        (let ((sum 0))
          (each b (bytes 1 2 3)
            (set sum (+ sum b)))
          sum)
    "#).unwrap();
    assert_eq!(result.as_int().unwrap(), 6);
}

#[test]
fn test_each_over_blob() {
    let result = eval_source(r#"
        (let ((sum 0))
          (each b (blob 10 20 30)
            (set sum (+ sum b)))
          sum)
    "#).unwrap();
    assert_eq!(result.as_int().unwrap(), 60);
}
```

---

## Section 5: Polymorphic `map`

### 5a. Update `src/primitives/higher_order_def.rs`

The current `map` definition (line 8-13) only handles lists via
`first`/`rest`/`empty?` recursion. Replace the `map_code` string with a
polymorphic version:

```rust
let map_code = r#"
    (def map (fn (f coll)
      (cond
        ((or (pair? coll) (empty? coll))
         (if (empty? coll)
           ()
           (cons (f (first coll)) (map f (rest coll)))))
        ((or (array? coll) (tuple? coll) (bytes? coll) (blob? coll))
         (letrec ((loop (fn (i acc)
                          (if (>= i (length coll))
                            (reverse acc)
                            (loop (+ i 1) (cons (f (get coll i)) acc))))))
           (loop 0 ())))
        ((or (string? coll) (buffer? coll))
         (letrec ((loop (fn (i acc)
                          (if (>= i (length coll))
                            (reverse acc)
                            (loop (+ i 1) (cons (f (string/char-at coll i)) acc))))))
           (loop 0 ())))
        (true (error :type-error "map: not a sequence")))))
"#;
```

**Note**: `map` always returns a list regardless of input type. This is
consistent with Lisp tradition.

**Note**: Same char-at name caveat as Section 4g. Use the registered name.

### 5b. Tests

Add to an appropriate integration test file:

```rust
#[test]
fn test_map_over_tuple() {
    let result = eval_source("(map (fn (x) (+ x 1)) [1 2 3])").unwrap();
    // map returns a list
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_int().unwrap(), 2);
    assert_eq!(vec[1].as_int().unwrap(), 3);
    assert_eq!(vec[2].as_int().unwrap(), 4);
}

#[test]
fn test_map_over_bytes() {
    let result = eval_source("(map (fn (b) (* b 2)) (bytes 1 2 3))").unwrap();
    let vec = result.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0].as_int().unwrap(), 2);
    assert_eq!(vec[1].as_int().unwrap(), 4);
    assert_eq!(vec[2].as_int().unwrap(), 6);
}
```

---

## Section 6: Part 1 — NaN-box Tag Reassignment

### Goal

Reorganize the NaN-box tag space to:
1. Group falsy values under a single tag prefix (enables single-comparison truthiness)
2. Free tag space for SSO by giving keyword and cpointer their own tag
3. Reserve `TAG_SSO` for Section 7 (short string optimization)

### 6a. New tag layout

| Tag | Name | Payload |
|-----|------|---------|
| 0x7FF8 | Integer | 48-bit signed int |
| 0x7FF9 | Falsy | 0=nil, 1=false |
| 0x7FFA | Empty-list | none (tag-only) |
| 0x7FFB | Heap pointer | 48-bit pointer |
| 0x7FFC | Truthy + symbol | sub-tagged: bit 47=0 for singletons (payload 0=true, 1=undefined), bit 47=1 for symbol (32-bit ID in lower bits) |
| 0x7FFD | NaN/Infinity | IEEE 754 special floats |
| 0x7FFE | Pointer values | 1-bit sub-tag (bit 47): 0=keyword (47-bit ptr), 1=cpointer (47-bit ptr) |
| 0x7FFF | SSO string | 6 bytes inline UTF-8 |

Keyword and cpointer get 47 bits of address space (no truncation). Symbol
gets 32 bits under 0x7FFC (plenty). Truthiness check is still
`(bits >> 48) != 0x7FF9`.

**File**: `src/value/repr/mod.rs`

Replace the tag constants section (lines 29-86) with:

```rust
// =============================================================================
// Tag Constants
// =============================================================================

/// Quiet NaN base - all tagged values have this prefix in upper 13 bits
pub(crate) const QNAN: u64 = 0x7FF8_0000_0000_0000;

/// Mask to check for quiet NaN (upper 13 bits)
pub(crate) const QNAN_MASK: u64 = 0xFFF8_0000_0000_0000;

/// Integer tag - uses QNAN exactly (0x7FF8), payload is 48-bit signed int
pub const TAG_INT: u64 = 0x7FF8_0000_0000_0000;
pub(crate) const TAG_INT_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Falsy tag - upper 16 bits = 0x7FF9
/// Nil = TAG_FALSY | 0, False = TAG_FALSY | 1
pub const TAG_FALSY: u64 = 0x7FF9_0000_0000_0000;
pub(crate) const TAG_FALSY_MASK: u64 = 0xFFFF_0000_0000_0000;
pub const TAG_NIL: u64 = 0x7FF9_0000_0000_0000;
pub const TAG_FALSE: u64 = 0x7FF9_0000_0000_0001;

/// Empty list tag - upper 16 bits = 0x7FFA
pub const TAG_EMPTY_LIST: u64 = 0x7FFA_0000_0000_0000;
pub(crate) const TAG_EMPTY_LIST_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Heap pointer tag - upper 16 bits = 0x7FFB
pub const TAG_POINTER: u64 = 0x7FFB_0000_0000_0000;
pub(crate) const TAG_POINTER_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Truthy + symbol tag - upper 16 bits = 0x7FFC
/// Bit 47 = 0: singleton (payload 0=true, 1=undefined)
/// Bit 47 = 1: symbol (bits 0-31 = symbol ID)
pub const TAG_TRUTHY: u64 = 0x7FFC_0000_0000_0000;
pub(crate) const TAG_TRUTHY_MASK: u64 = 0xFFFF_0000_0000_0000;
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0000;
pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TRUTHY_SYMBOL_BIT: u64 = 1u64 << 47; // bit 47 = symbol sub-tag
pub(crate) const SYMBOL_ID_MASK: u64 = 0xFFFF_FFFF; // bits 0-31

/// NaN/Infinity tag - upper 16 bits = 0x7FFD
pub const TAG_NAN: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const TAG_NAN_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Pointer values tag - upper 16 bits = 0x7FFE
/// Bit 47 = 0: keyword (bits 0-46 = interned string pointer)
/// Bit 47 = 1: cpointer (bits 0-46 = raw C pointer address)
pub const TAG_PTRVAL: u64 = 0x7FFE_0000_0000_0000;
pub(crate) const TAG_PTRVAL_MASK: u64 = 0xFFFF_0000_0000_0000;
pub(crate) const PTRVAL_CPOINTER_BIT: u64 = 1u64 << 47; // bit 47 = cpointer sub-tag
pub(crate) const PTRVAL_PAYLOAD_MASK: u64 = (1u64 << 47) - 1; // bits 0-46

/// SSO (Short String Optimization) tag - upper 16 bits = 0x7FFF
/// Payload: up to 6 UTF-8 bytes packed into bits 0-47, zero-padded
pub const TAG_SSO: u64 = 0x7FFF_0000_0000_0000;
pub(crate) const TAG_SSO_MASK: u64 = 0xFFFF_0000_0000_0000;

/// Mask for 48-bit payload extraction
pub(crate) const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Maximum 48-bit signed integer (2^47 - 1)
pub const INT_MAX: i64 = 0x7FFF_FFFF_FFFF;

/// Minimum 48-bit signed integer (-2^47)
pub const INT_MIN: i64 = -0x8000_0000_0000;
```

Remove the old `TAG_SYMBOL`, `TAG_SYMBOL_MASK`, `TAG_KEYWORD`, `TAG_KEYWORD_MASK`,
`TAG_CPOINTER`, `TAG_CPOINTER_MASK` constants entirely.

### 6b. Update predicates in `src/value/repr/mod.rs`

Replace the predicate methods:

```rust
/// Check if this is the nil value.
#[inline]
pub fn is_nil(&self) -> bool {
    self.0 == TAG_NIL
}

/// Check if this is an empty list.
#[inline]
pub fn is_empty_list(&self) -> bool {
    self.0 == TAG_EMPTY_LIST
}

/// Check if this is the undefined sentinel value.
#[inline]
pub fn is_undefined(&self) -> bool {
    self.0 == TAG_UNDEFINED
}

/// Check if this is a boolean (true or false).
#[inline]
pub fn is_bool(&self) -> bool {
    self.0 == TAG_TRUE || self.0 == TAG_FALSE
}

/// Check if this is an integer.
#[inline]
pub fn is_int(&self) -> bool {
    (self.0 & TAG_INT_MASK) == TAG_INT
}

/// Check if this is a float (not a tagged value).
#[inline]
pub fn is_float(&self) -> bool {
    let tag = self.0 & QNAN_MASK;
    tag != QNAN || (self.0 & TAG_NAN_MASK) == TAG_NAN
}

/// Check if this is a number (int or float).
#[inline]
pub fn is_number(&self) -> bool {
    self.is_int() || self.is_float()
}

/// Check if this is a symbol.
#[inline]
pub fn is_symbol(&self) -> bool {
    (self.0 & TAG_TRUTHY_MASK) == TAG_TRUTHY
        && (self.0 & TRUTHY_SYMBOL_BIT) != 0
}

/// Check if this is a keyword.
#[inline]
pub fn is_keyword(&self) -> bool {
    (self.0 & TAG_PTRVAL_MASK) == TAG_PTRVAL
        && (self.0 & PTRVAL_CPOINTER_BIT) == 0
}

/// Check if this is a raw C pointer.
#[inline]
pub fn is_pointer(&self) -> bool {
    (self.0 & TAG_PTRVAL_MASK) == TAG_PTRVAL
        && (self.0 & PTRVAL_CPOINTER_BIT) != 0
}

/// Check if this is a heap pointer.
#[inline]
pub fn is_heap(&self) -> bool {
    (self.0 & TAG_POINTER_MASK) == TAG_POINTER
}

/// Check if this value is truthy (everything except nil and false).
#[inline]
pub fn is_truthy(&self) -> bool {
    debug_assert!(
        !self.is_undefined(),
        "UNDEFINED leaked into truthiness check"
    );
    (self.0 >> 48) != 0x7FF9
}
```

**Key change**: `is_truthy()` is now a single shift+compare. This is the
primary motivation for the tag reassignment.

### 6c. Update constructors in `src/value/repr/constructors.rs`

Update the import line to use new constants:

```rust
use super::{
    Value, INT_MAX, INT_MIN, PAYLOAD_MASK, QNAN, QNAN_MASK,
    TRUTHY_SYMBOL_BIT, SYMBOL_ID_MASK, TAG_INT, TAG_NAN, TAG_POINTER,
    TAG_TRUTHY, TAG_PTRVAL, PTRVAL_CPOINTER_BIT, PTRVAL_PAYLOAD_MASK,
};
```

Replace `Value::symbol`:
```rust
#[inline]
pub fn symbol(id: u32) -> Self {
    Value(TAG_TRUTHY | TRUTHY_SYMBOL_BIT | (id as u64))
}
```

Replace `Value::keyword`:
```rust
#[inline]
pub fn keyword(name: &str) -> Self {
    use crate::value::intern::intern_string;
    let ptr = intern_string(name) as *const ();
    let addr = ptr as u64;
    assert!(
        addr & !PTRVAL_PAYLOAD_MASK == 0,
        "Keyword pointer exceeds 47-bit address space"
    );
    Value(TAG_PTRVAL | addr)
}
```

Replace `Value::pointer`:
```rust
#[inline]
pub fn pointer(addr: usize) -> Self {
    if addr == 0 {
        return Self::NIL;
    }
    let addr_u64 = addr as u64;
    assert!(
        addr_u64 & !PTRVAL_PAYLOAD_MASK == 0,
        "C pointer exceeds 47-bit address space"
    );
    Value(TAG_PTRVAL | PTRVAL_CPOINTER_BIT | (addr_u64 & PTRVAL_PAYLOAD_MASK))
}
```

**Note**: Keyword and cpointer constructors use `assert!` (not
`debug_assert!`) for pointer range checks. These are safety-critical —
a truncated pointer is a use-after-free waiting to happen.

### 6d. Update accessors in `src/value/repr/accessors.rs`

Update the import line:
```rust
use super::{Value, PAYLOAD_MASK, TAG_FALSE, TAG_NAN, TAG_NAN_MASK, TAG_TRUE,
            TRUTHY_SYMBOL_BIT, SYMBOL_ID_MASK, TAG_TRUTHY_MASK, TAG_TRUTHY,
            TAG_PTRVAL_MASK, TAG_PTRVAL, PTRVAL_CPOINTER_BIT, PTRVAL_PAYLOAD_MASK};
```

Replace `as_symbol`:
```rust
#[inline]
pub fn as_symbol(&self) -> Option<u32> {
    if self.is_symbol() {
        Some((self.0 & SYMBOL_ID_MASK) as u32)
    } else {
        None
    }
}
```

Replace `as_pointer`:
```rust
#[inline]
pub fn as_pointer(&self) -> Option<usize> {
    if self.is_pointer() {
        Some((self.0 & PTRVAL_PAYLOAD_MASK) as usize)
    } else {
        None
    }
}
```

Replace `as_keyword_name`:
```rust
#[inline]
pub fn as_keyword_name(&self) -> Option<&str> {
    if self.is_keyword() {
        let ptr = (self.0 & PTRVAL_PAYLOAD_MASK) as *const crate::value::heap::HeapObject;
        match unsafe { &*ptr } {
            crate::value::heap::HeapObject::String(s) => Some(s),
            _ => None,
        }
    } else {
        None
    }
}
```

### 6e. Update JIT files

**File**: `src/jit/translate.rs`

Line 16 imports: update to use new constants. The file imports
`TAG_EMPTY_LIST`, `TAG_FALSE`, `TAG_INT`, `TAG_NIL`, `TAG_TRUE` from
`crate::value::repr`. These are still valid constant names — only their
bit patterns changed. **No import changes needed.**

Lines 643-651 (truthiness check in JIT): Replace the two-comparison pattern:
```rust
// Old:
let nil = builder.ins().iconst(I64, TAG_NIL as i64);
let false_val = builder.ins().iconst(I64, TAG_FALSE as i64);
let not_nil = builder.ins().icmp(IntCC::NotEqual, val, nil);
let not_false = builder.ins().icmp(IntCC::NotEqual, val, false_val);
let is_truthy = builder.ins().band(not_nil, not_false);

// New (single comparison: shift upper 16 bits, compare to 0x7FF9):
let shifted = builder.ins().ushr_imm(val, 48);
let falsy_tag = builder.ins().iconst(I64, 0x7FF9_i64);
let is_truthy = builder.ins().icmp(IntCC::NotEqual, shifted, falsy_tag);
```

Apply this pattern at **all** truthiness check sites in `translate.rs`
(lines ~643-651 and ~903-907). Import `TAG_FALSY`.

Line 674: `LirConst::Nil => TAG_NIL` — no change needed (TAG_NIL is still
a valid constant, just different bit pattern).

Lines 676-677: `TAG_TRUE`, `TAG_FALSE` — same, still valid.

**File**: `src/jit/dispatch.rs`

Line 7 import: `use crate::value::repr::TAG_NIL;` — no change needed.

All uses of `TAG_NIL` as a sentinel return value are fine — the constant
name is the same, only the bit pattern changed.

**File**: `src/jit/runtime.rs`

Line 12 import: update to include `TAG_FALSY`.

Line 286: `Value::bool(a == TAG_NIL)` — no change needed.

Lines 291-292 (`elle_jit_is_truthy`): Replace:
```rust
// Old:
pub extern "C" fn elle_jit_is_truthy(a: u64) -> u64 {
    Value::bool(a != TAG_NIL && a != TAG_FALSE).to_bits()
}

// New:
pub extern "C" fn elle_jit_is_truthy(a: u64) -> u64 {
    Value::bool((a >> 48) != 0x7FF9).to_bits()
}
```

Import `TAG_FALSY` in the import line.

### 6f. Update equality in `src/value/repr/traits.rs`

No changes needed. The `PartialEq` impl compares bits for non-heap values
(`self.0 == other.0`), which works regardless of bit patterns. Heap
comparison delegates to `HeapObject` matching.

### 6g. Update `src/value/repr/tests.rs`

Any tests that assert specific bit patterns will need updating. Read the
file and update hardcoded `0x7FFC...` patterns to match the new layout.

### 6h. Verify

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -- --check
cargo build --release && ./target/release/elle elle-doc/generate.lisp
```

The JIT tests are critical here — they exercise the truthiness fast path.

---

## Section 7: Part 2 — SSO (Short String Optimization)

### Goal

Strings ≤6 UTF-8 bytes are stored inline in the NaN-boxed Value (no heap
allocation). Strings >6 bytes use the existing heap interning path.

### 7a. Update `Value::string()` constructor

**File**: `src/value/repr/constructors.rs`

Replace the `string` method:

```rust
/// Create a string value.
/// Strings ≤6 UTF-8 bytes are stored inline (SSO).
/// Strings >6 bytes are heap-interned.
#[inline]
pub fn string(s: impl Into<Box<str>>) -> Self {
    let boxed: Box<str> = s.into();
    let bytes = boxed.as_bytes();
    if bytes.len() <= 6 {
        // Pack into SSO: TAG_SSO | bytes in little-endian order
        let mut bits: u64 = 0;
        for (i, &b) in bytes.iter().enumerate() {
            bits |= (b as u64) << (i * 8);
        }
        Value(super::TAG_SSO | bits)
    } else {
        use crate::value::intern::intern_string;
        let ptr = intern_string(&boxed) as *const ();
        Self::from_heap_ptr(ptr)
    }
}
```

### 7b. Add `with_string` accessor and update `is_string`

**File**: `src/value/repr/accessors.rs`

Replace `is_string`:
```rust
/// Check if this is a string (SSO or heap).
#[inline]
pub fn is_string(&self) -> bool {
    use crate::value::heap::HeapTag;
    (self.0 & super::TAG_SSO_MASK) == super::TAG_SSO
        || self.heap_tag() == Some(HeapTag::String)
}
```

Delete `as_string` entirely and replace with `with_string`:
```rust
/// Access string contents via closure. Works for both SSO and heap strings.
/// Returns None if this is not a string.
#[inline]
pub fn with_string<R>(&self, f: impl FnOnce(&str) -> R) -> Option<R> {
    if (self.0 & super::TAG_SSO_MASK) == super::TAG_SSO {
        let payload = self.0 & super::PAYLOAD_MASK;
        let mut buf = [0u8; 6];
        for i in 0..6 {
            buf[i] = ((payload >> (i * 8)) & 0xFF) as u8;
        }
        // Find length: first zero byte, or 6 if all non-zero
        let len = buf.iter().position(|&b| b == 0).unwrap_or(6);
        // SAFETY: Value::string() only creates SSO from valid UTF-8
        let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
        Some(f(s))
    } else {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::String(s) => Some(f(s)),
            _ => None,
        }
    }
}
```

**IMPORTANT**: `as_string()` is deleted, not deprecated. All ~93 call sites
will get compile errors, forcing migration to `with_string()`. This is
intentional — AGENTS.md says "Do not add backward compatibility machinery."
A silent shim that returns `None` for SSO strings would be worse than a
compile error: it would produce subtle runtime bugs.

### 7c. Convert call sites

There are ~93 call sites of `as_string()` across the codebase. Deleting
`as_string()` will produce compile errors at every one, which is the
desired forcing function. The mechanical conversion patterns:

**Pattern 1**: `if let Some(s) = v.as_string() { expr_using_s }`
→ `if let Some(r) = v.with_string(|s| { expr_using_s }) { r }`

But if `expr_using_s` contains early returns, this won't work because you
can't return from inside a closure. For those cases:

**Pattern 2** (early return in primitives):
```rust
// Old:
match args[0].as_string() {
    Some(s) => { /* use s, possibly return early */ },
    None => { /* error */ },
}

// New — extract to local String first:
let s_owned: String;
let s_ref: &str;
if let Some(r) = args[0].with_string(|s| s.to_string()) {
    s_owned = r;
    s_ref = &s_owned;
} else {
    return (SIG_ERROR, error_val("type-error", ...));
}
// Now use s_ref
```

This allocates for SSO strings, which defeats the purpose. Better pattern:

**Pattern 3** (restructure to avoid early return inside closure):
```rust
// Old:
if let Some(s) = args[0].as_string() {
    return (SIG_OK, Value::string(s.to_uppercase().as_str()));
}

// New:
if args[0].is_string() {
    return args[0].with_string(|s| {
        (SIG_OK, Value::string(s.to_uppercase().as_str()))
    }).unwrap();
}
```

**Pattern 4**: `v.as_string().is_some()` → `v.is_string()`

**Pattern 5**: `v.as_string() == Some("foo")` → `v.with_string(|s| s == "foo") == Some(true)`

**Files to convert** (by module, with approximate call count):

| File | Calls | Notes |
|------|-------|-------|
| `src/primitives/string.rs` | ~15 | Heavy user. Most are `match args[N].as_string()` |
| `src/primitives/file_io.rs` | ~18 | All are `if let Some(path) = args[0].as_string()` |
| `src/primitives/table.rs` | ~3 | In `value_to_table_key` and `prim_get` string branch |
| `src/primitives/list.rs` | ~5 | In `prim_length`, `prim_append`, `prim_concat` |
| `src/primitives/display.rs` | ~3 | In `prim_display`, `prim_print` |
| `src/primitives/bytes.rs` | ~2 | In `prim_string_to_bytes`, `prim_string_to_blob` |
| `src/primitives/crypto.rs` | ~1 | In `extract_byte_data` |
| `src/primitives/buffer.rs` | ~1 | In `prim_string_to_buffer` |
| `src/primitives/convert.rs` | ~4 | In number parsing |
| `src/primitives/ffi.rs` | ~3 | In `prim_ffi_open`, `prim_ffi_lookup` |
| `src/primitives/json/*.rs` | ~4 | In serializer and parser |
| `src/primitives/debugging.rs` | ~2 | |
| `src/primitives/meta.rs` | ~1 | |
| `src/primitives/read.rs` | ~2 | |
| `src/primitives/module_loading.rs` | ~2 | |
| `src/primitives/path.rs` | ~6 | |
| `src/primitives/structs.rs` | ~1 | |
| `src/primitives/type_check.rs` | ~1 | Use `is_string()` |
| `src/value/display.rs` | ~2 | In Display and Debug impls |
| `src/value/send.rs` | ~1 | In `from_value` |
| `src/value/error.rs` | ~2 | |
| `src/value/types.rs` | ~1 | |
| `src/value/repr/tests.rs` | ~1 | |
| `src/value/intern.rs` | ~1 | |
| `src/syntax/convert.rs` | ~1 | |
| `src/vm/data.rs` | ~1 | |
| `src/vm/signal.rs` | ~4 | |
| `src/ffi/marshal.rs` | ~2 | |

### 7d. Update `src/value/display.rs`

Replace `as_string` calls with `with_string`:

```rust
// Display impl, line ~64:
// Old: if let Some(s) = self.as_string() { return write!(f, "{}", s); }
// New:
if let Some(()) = self.with_string(|s| { write!(f, "{}", s).ok(); }) {
    // with_string returned Some, meaning it was a string
    // But we need the fmt::Result. Restructure:
}
```

Better approach for Display:
```rust
if self.is_string() {
    return self.with_string(|s| write!(f, "{}", s)).unwrap_or(Ok(()));
}
```

Same for Debug (line ~272):
```rust
if self.is_string() {
    return self.with_string(|s| write!(f, "\"{}\"", s)).unwrap_or(Ok(()));
}
```

### 7e. Update `src/value/send.rs`

In `from_value`, the string check (line ~87):
```rust
// Old: if let Some(s) = value.as_string() { return Ok(SendValue::String(s.to_string())); }
// New:
if let Some(s) = value.with_string(|s| s.to_string()) {
    return Ok(SendValue::String(s));
}
```

### 7f. Update `src/value/repr/traits.rs`

SSO vs SSO comparison: already works via `self.0 == other.0` (bit comparison).

SSO vs heap: If `Value::string()` always chooses SSO for ≤6 bytes, then
the same string content will always be SSO or always be heap — never both.
So cross-type comparison returning `false` is correct.

**Exception**: `SendValue::into_value()` in `src/value/send.rs` reconstructs
strings via `alloc(HeapObject::String(...))`, intentionally bypassing the
thread-local interner for thread safety. When SSO lands, this needs a
split path:

- For SSO-length strings (≤6 bytes): use `Value::string()` — the SSO path
  packs bytes inline, no interning involved, so it's thread-safe.
- For longer strings: keep the current `alloc(HeapObject::String(boxed))`
  path to avoid hitting the thread-local interner from a foreign thread.

Add a `Value::string_no_intern()` constructor (or similar) for the long-string
case:

```rust
// In constructors.rs:
/// Create a heap string without interning. Used by SendValue::into_value()
/// to avoid thread-local interner issues when reconstructing values on
/// a different thread.
#[inline]
pub fn string_no_intern(s: impl Into<Box<str>>) -> Self {
    let boxed: Box<str> = s.into();
    let bytes = boxed.as_bytes();
    if bytes.len() <= 6 && !bytes.contains(&0) {
        // SSO path — no interning, thread-safe
        let mut bits: u64 = 0;
        for (i, &b) in bytes.iter().enumerate() {
            bits |= (b as u64) << (i * 8);
        }
        Value(super::TAG_SSO | bits)
    } else {
        // Heap alloc without interning
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::String(boxed))
    }
}
```

Then in `send.rs`:
```rust
SendValue::String(s) => Value::string_no_intern(s),
```

This preserves the thread-safety invariant while getting SSO for short
strings.

### 7g. SSO edge case: NUL bytes

SSO uses zero bytes as padding/length detection. A string containing a NUL
byte (`\0`) would be misinterpreted. However, Elle strings are UTF-8, and
while `\0` is valid UTF-8, it's extremely rare in practice. Two options:

1. **Disallow**: `Value::string()` falls back to heap for strings containing `\0`.
   Add: `if bytes.len() <= 6 && !bytes.contains(&0) { /* SSO */ }`
2. **Store length**: Use one of the 6 payload bytes as a length byte, leaving
   5 bytes for content.

**Recommendation**: Option 1. It's simpler and `\0` in strings is vanishingly
rare. Document this in the SSO tag comment.

### 7h. Tests

Add to `src/value/repr/tests.rs`:

```rust
#[test]
fn test_sso_short_string() {
    let v = Value::string("hi");
    assert!(v.is_string());
    assert!(!v.is_heap()); // SSO, not heap
    assert_eq!(v.with_string(|s| s.to_string()), Some("hi".to_string()));
}

#[test]
fn test_sso_empty_string() {
    let v = Value::string("");
    assert!(v.is_string());
    assert!(!v.is_heap());
    assert_eq!(v.with_string(|s| s.to_string()), Some(String::new()));
}

#[test]
fn test_sso_six_byte_string() {
    let v = Value::string("abcdef");
    assert!(v.is_string());
    assert!(!v.is_heap());
    assert_eq!(v.with_string(|s| s.to_string()), Some("abcdef".to_string()));
}

#[test]
fn test_heap_seven_byte_string() {
    let v = Value::string("abcdefg");
    assert!(v.is_string());
    assert!(v.is_heap()); // Too long for SSO
    assert_eq!(v.with_string(|s| s.to_string()), Some("abcdefg".to_string()));
}

#[test]
fn test_sso_equality() {
    let a = Value::string("hi");
    let b = Value::string("hi");
    assert_eq!(a, b);
    assert_eq!(a.to_bits(), b.to_bits()); // Same bit pattern
}

#[test]
fn test_sso_nul_byte_falls_back_to_heap() {
    let v = Value::string("a\0b");
    assert!(v.is_string());
    assert!(v.is_heap()); // Contains NUL, falls back to heap
}
```

### 7i. Verify

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -- --check
cargo build --release && ./target/release/elle elle-doc/generate.lisp
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

The elle-doc generation is critical — it exercises string-heavy runtime paths.

---

## Ordering

| Step | Section | Depends on | Risk |
|------|---------|------------|------|
| 1 | Section 1 (OOB fix) | nothing | trivial |
| 2 | Section 2 (remaining primitives) | nothing | low |
| 3 | Section 3 (keys/values/del) | nothing | no-op |
| 4 | Section 4 (each macro) | nothing | medium — removing a special form |
| 5 | Section 5 (polymorphic map) | nothing | low |
| 6 | Section 6 (tag reassignment) | 1-5 stable | high — touches everything |
| 7 | Section 7 (SSO) | 6 | high — 94 call sites |

Each section gets its own commit. Run the full verification suite after each.
