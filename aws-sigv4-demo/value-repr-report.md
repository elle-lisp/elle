# Value Representation Report

Reconnaissance of Elle's NaN-boxing scheme, string representation, and
feasibility of inline short strings.

## 1. The NaN-boxing Scheme

### Layout

`Value` is a `#[repr(transparent)]` wrapper around `u64` (8 bytes, `Copy`).

- **File**: `src/value/repr/mod.rs`, line 98-100
- **Size assertion**: line 103: `const _: () = assert!(std::mem::size_of::<Value>() == 8);`

IEEE 754 quiet NaN prefix occupies the upper 13 bits (`0x7FF8` in bits 51-63).
The scheme uses the **upper 16 bits** as type tags and the **lower 48 bits**
as payload.

### Tag Allocation (upper 16 bits)

From `src/value/repr/mod.rs` lines 34-76:

| Tag (hex) | Constant       | Payload (48 bits)              | Notes |
|-----------|----------------|--------------------------------|-------|
| `0x7FF8`  | `TAG_INT`      | 48-bit signed integer          | Overlaps QNAN base exactly |
| `0x7FF9`  | `TAG_SYMBOL`   | 32-bit symbol ID (upper 16 of payload unused) | |
| `0x7FFA`  | `TAG_KEYWORD`  | 48-bit interned HeapObject pointer | Points to `HeapObject::String` |
| `0x7FFB`  | `TAG_POINTER`  | 48-bit heap pointer            | All heap-allocated values |
| `0x7FFC`  | (singletons)   | Small distinguishing payload   | nil/true/false/empty-list/undefined |
| `0x7FFD`  | `TAG_NAN`      | Upper 16 bits of original float | For NaN/Infinity storage |
| `0x7FFE`  | `TAG_CPOINTER` | 48-bit raw C pointer address   | FFI raw pointers |
| `0x7FFF`  | (unused)       | —                              | **Available** |

Singleton values sharing the `0x7FFC` tag (`src/value/repr/mod.rs` lines 40-52):

| Bits (full 64) | Value |
|-----------------|-------|
| `0x7FFC_0000_0000_0000` | `TAG_NIL` |
| `0x7FFC_0000_0000_0001` | `TAG_FALSE` |
| `0x7FFC_0000_0000_0002` | `TAG_TRUE` |
| `0x7FFC_0000_0000_0003` | `TAG_EMPTY_LIST` |
| `0x7FFC_0000_0000_0004` | `TAG_UNDEFINED` |

Floats: any `f64` whose upper 13 bits are NOT the quiet NaN prefix
(`0xFFF8` mask check, line 37). Non-quiet-NaN floats are stored as raw
`f64` bits directly.

### Bit budget

Tags occupy the upper 16 bits (bits 48-63). The quiet NaN range for tags
spans `0x7FF8` through `0x7FFF` (8 values). Currently used: 7 of 8.

**`0x7FFF` is unused and available as a new tag.**

Additionally, within the sign-bit-set range (negative NaN space), tags
`0xFFF8` through `0xFFFF` are potentially available, though these would
set the sign bit. The current code uses `QNAN_MASK = 0xFFF8_0000_0000_0000`
(line 37) to detect tagged values, which masks the sign bit. Using sign-bit-set
tags would require careful analysis of `is_float()` (line 153-156).

## 2. Current String Representation

### Storage: Interned `HeapObject::String(Box<str>)` via `Rc`

- **Heap object variant**: `src/value/heap.rs` line 66: `String(Box<str>)`
- **Constructor**: `src/value/repr/constructors.rs` lines 122-128:

```rust
pub fn string(s: impl Into<Box<str>>) -> Self {
    use crate::value::intern::intern_string;
    let boxed: Box<str> = s.into();
    let ptr = intern_string(&boxed) as *const ();
    Self::from_heap_ptr(ptr)
}
```

Strings are **not** `Rc<String>` or `Rc<str>`. They are:
1. Interned via `intern_string()` → returns `*const HeapObject`
2. The interner stores `Rc<HeapObject>` in a thread-local `HashMap<Box<str>, Rc<HeapObject>>`
3. The `Value` stores the raw pointer (48-bit) with `TAG_POINTER` (`0x7FFB`)

**Interning module**: `src/value/intern.rs` lines 17-53

```rust
struct StringInterner {
    strings: HashMap<Box<str>, Rc<HeapObject>>,
}

fn intern(&mut self, s: &str) -> *const HeapObject {
    if let Some(rc) = self.strings.get(s) {
        return Rc::as_ptr(rc);
    }
    let rc = Rc::new(HeapObject::String(s.into()));
    let ptr = Rc::as_ptr(&rc);
    self.strings.insert(s.into(), rc);
    ptr
}
```

### Accessor

`src/value/repr/accessors.rs` lines 214-224:

```rust
pub fn as_string(&self) -> Option<&str> {
    use crate::value::heap::{deref, HeapObject};
    if !self.is_heap() {
        return None;
    }
    match unsafe { deref(*self) } {
        HeapObject::String(s) => Some(s),
        _ => None,
    }
}
```

### Type check

`src/value/repr/accessors.rs` lines 114-118:

```rust
pub fn is_string(&self) -> bool {
    use crate::value::heap::HeapTag;
    self.heap_tag() == Some(HeapTag::String)
}
```

This dereferences the heap pointer and checks the `HeapTag` discriminant —
a pointer chase on every `is_string()` call.

## 3. Current Buffer Representation

- **Heap object variant**: `src/value/heap.rs` line 87: `Buffer(RefCell<Vec<u8>>)`
- **Constructor**: `src/value/repr/constructors.rs` lines 216-221:

```rust
pub fn buffer(bytes: Vec<u8>) -> Self {
    use crate::value::heap::{alloc, HeapObject};
    use std::cell::RefCell;
    alloc(HeapObject::Buffer(RefCell::new(bytes)))
}
```

Buffers are heap-allocated via `alloc()` (same as all heap values), wrapped
in `RefCell` for mutability. Not interned. Stored as `TAG_POINTER` + heap
pointer, same as strings.

### Accessor

`src/value/repr/accessors.rs` lines 351-362:

```rust
pub fn as_buffer(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
    ...
    match unsafe { deref(*self) } {
        HeapObject::Buffer(b) => Some(b),
        _ => None,
    }
}
```

## 4. Existing Inline Value Types

Types stored entirely in the 64-bit Value without heap allocation:

| Type | How stored | Payload bits used |
|------|-----------|-------------------|
| `nil` | Singleton constant `0x7FFC_0000_0000_0000` | 0 (exact bit pattern) |
| `true` | Singleton constant `0x7FFC_0000_0000_0002` | 0 (exact bit pattern) |
| `false` | Singleton constant `0x7FFC_0000_0000_0001` | 0 (exact bit pattern) |
| `empty_list` | Singleton constant `0x7FFC_0000_0000_0003` | 0 (exact bit pattern) |
| `undefined` | Singleton constant `0x7FFC_0000_0000_0004` | 0 (exact bit pattern) |
| Integer | `TAG_INT` \| sign-extended 48-bit int | 48 bits |
| Float | Raw `f64` bits (non-quiet-NaN range) | 64 bits (no tag) |
| NaN/Inf | `TAG_NAN` \| upper 16 bits of float | 16 bits of payload |
| Symbol | `TAG_SYMBOL` \| 32-bit symbol ID | 32 bits |
| Keyword | `TAG_KEYWORD` \| 48-bit interned pointer | 48 bits (pointer) |
| C Pointer | `TAG_CPOINTER` \| 48-bit raw address | 48 bits |

Keywords are "semi-inline" — the tag is immediate but the payload is a pointer
to an interned `HeapObject::String`. The keyword itself doesn't heap-allocate
a new `Rc`; it reuses the interned string's `Rc` pointer.

## 5. Tag Exhaustion Analysis

### Available tag space

The 16-bit tag occupies bits 48-63. The quiet NaN prefix requires bits 48-60
to be `0x7FF8` (value `0x7FF` in bits 52-62, bit 51 = 1 for quiet NaN).

Effective tag range: `0x7FF8` through `0x7FFF` — **8 possible tags**.

| Tag | Used by |
|-----|---------|
| `0x7FF8` | Integer |
| `0x7FF9` | Symbol |
| `0x7FFA` | Keyword |
| `0x7FFB` | Heap Pointer |
| `0x7FFC` | Singletons (nil/bool/empty-list/undefined) |
| `0x7FFD` | NaN/Infinity |
| `0x7FFE` | C Pointer |
| `0x7FFF` | **UNUSED** |

**One tag value (`0x7FFF`) is available.** This is enough for one new inline
type, not three. Adding `bytes`, `blob`, AND `short_string` as three separate
tags is not possible within the current 3-bit tag scheme.

### Expanding tag space

The singletons tag (`0x7FFC`) uses only 5 of its 2^48 payload values. The
remaining ~281 trillion payload values are wasted. One could sub-tag within
`0x7FFC` (e.g., payloads 0-4 are singletons, payloads 5+ are something else),
but this would complicate the singleton fast-path checks.

Another option: use the sign bit. Negative quiet NaN values (`0xFFF8` through
`0xFFFF`) are a mirror of the positive range, giving 8 more tags. However,
`is_float()` (line 153-156) currently treats the sign bit as part of float
detection; using negative NaN tags would require adjusting this logic.

## 6. The `get` Dispatch on Strings

### `prim_get` in `src/primitives/table.rs`

Lines 309-336 handle the string case:

```rust
// String (immutable character sequence)
if let Some(s) = args[0].as_string() {
    let index = match args[1].as_int() {
        Some(i) => i,
        None => { return (SIG_ERROR, ...) }
    };
    if index < 0 {
        return (SIG_OK, default);
    }
    match s.chars().nth(index as usize) {
        Some(ch) => {
            let ch_str = ch.to_string();
            return (SIG_OK, Value::string(ch_str.as_str()));
        }
        None => return (SIG_OK, default),
    }
}
```

**Yes, it allocates.** `ch.to_string()` creates a heap `String`, then
`Value::string(ch_str.as_str())` interns it (HashMap lookup + possible
`Rc<HeapObject>` allocation). For single ASCII characters, this is a
1-byte string that goes through full interning.

### `prim_char_at` in `src/primitives/string.rs`

Lines 240-241:

```rust
match s.chars().nth(index) {
    Some(c) => (SIG_OK, Value::string(c.to_string())),
```

Same pattern: `char.to_string()` → heap allocation → interning.

## 7. Equality on Strings

### `PartialEq` implementation

`src/value/repr/traits.rs` lines 5-97.

The equality check is:

1. **Lines 10-11**: If both values are non-heap, compare bits directly:
   ```rust
   if !self.is_heap() && !other.is_heap() {
       return self.0 == other.0;
   }
   ```

2. **Lines 14-17**: If one is heap and the other isn't, return false.

3. **Lines 20-26**: Both heap — dereference and match on `HeapObject` variant:
   ```rust
   (HeapObject::String(s1), HeapObject::String(s2)) => s1 == s2,
   ```

**No pointer comparison shortcut.** The code does NOT check
`self.0 == other.0` (bit-level pointer equality) before dereferencing
for heap values. However, since strings are interned, two `Value`s
containing the same string content will have the **same pointer** (same
`*const HeapObject`), so they'll have the same 64-bit bit pattern. The
first check (`!self.is_heap() && !other.is_heap()`) doesn't catch this
because both ARE heap values. They fall through to the dereference path.

**Interning makes string equality correct but suboptimal.** Because
interned strings share the same pointer, `self.0 == other.0` would be
true for equal strings. But the code dereferences both pointers and does
`Box<str>` comparison anyway.

### Impact of inline short strings

If short strings were stored inline (no heap pointer), two cases arise:
1. **Short == Short**: bit comparison (`self.0 == other.0`) — fast, correct.
2. **Short == Long (heap)**: one is non-heap, one is heap → lines 14-17
   return `false`. This is **correct** only if the short string's content
   genuinely differs from the long string's. But a 6-byte string could
   exist as both an inline short string AND an interned heap string if
   construction paths differ.

This means `Value::string("hi")` must consistently produce the same
representation (inline or heap) to avoid breaking equality. The
constructor would need to always choose inline for short strings.

## 8. Existing SSO (Small String Optimization)

**None.** There is no small string optimization anywhere in the codebase.

- `grep` for `short.string`, `inline.string`, `small.string`, `SSO`, `sso`
  across all of `src/` returned zero results.
- All strings go through the same path: `Value::string()` → `intern_string()`
  → `HashMap` lookup → `Rc<HeapObject::String(Box<str>)>`.
- Single-character strings from `get` and `char-at` also go through this path.

### What interning provides instead

Interning gives O(1) equality (in theory — see section 7 on the missed
pointer-equality shortcut) and deduplication. For frequently-used short
strings (empty string, single characters, common identifiers), interning
avoids repeated allocation but still requires a HashMap lookup on every
`Value::string()` call.

## Summary of Key Facts

| Question | Answer |
|----------|--------|
| Available tag slots | 1 unused (`0x7FFF`) out of 8 total |
| String storage | `HeapObject::String(Box<str>)` via `Rc`, interned |
| Buffer storage | `HeapObject::Buffer(RefCell<Vec<u8>>)` via `Rc`, not interned |
| Inline types today | int (i48), symbol (u32), keyword (ptr), float (f64), singletons, C pointer |
| Short string optimization | None |
| `get` on string allocates? | Yes — `char.to_string()` + `Value::string()` interning |
| String equality | Deref + `Box<str>` comparison (no pointer shortcut for heap values) |
| Room for 3 new tags? | No — only 1 tag available. Would need sign-bit expansion or sub-tagging |
