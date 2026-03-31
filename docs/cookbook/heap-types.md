# Adding a New Heap Type


A heap type is a new kind of runtime value stored behind a tagged-union
pointer. Use the `LStringMut` type as a reference — it's a recent, clean
example.

### Files to modify (in order)

1. **`src/value/heap.rs`** — Add variant to `HeapObject` enum, add tag to
   `HeapTag` enum, add arms to `tag()`, `type_name()`, and `Debug`.

2. **`src/value/repr/constructors.rs`** — Add `Value::my_type(...)` constructor.

3. **`src/value/repr/accessors.rs`** — Add `is_my_type()` predicate and
   `as_my_type()` accessor.

4. **`src/value/display.rs`** — Add `Display` and `Debug` formatting arms.

5. **`src/value/repr/traits.rs`** — Add `PartialEq` arm for the new type.

6. **`src/value/send.rs`** — Add `SendValue` variant (if sendable) or
   rejection arm (if not).

7. **`src/primitives/json/serializer.rs`** — Add arm to `serialize_value`
   (exhaustive `HeapTag` match).

8. **`src/formatter/core.rs`** — Add arm to `format_value` (exhaustive
   `HeapObject` match).

9. **`src/syntax/convert.rs`** — Update `Syntax::from_value()` if the type
   can appear in macro results (Value → Syntax conversion).

### Step by step

**Step 1: `src/value/heap.rs`** — Three changes:

```rust
// In HeapTag enum — assign next available discriminant:
pub enum HeapTag {
    // ... existing ...
    MyType = 22,  // next after LStringMut = 21
}

// In HeapObject enum:
pub enum HeapObject {
    // ... existing ...
    /// Description of my type
    MyType(MyTypeData),
}

// In HeapObject::tag():
HeapObject::MyType(_) => HeapTag::MyType,

// In HeapObject::type_name():
HeapObject::MyType(_) => "my-type",

// In HeapObject Debug impl:
HeapObject::MyType(_) => write!(f, "<my-type>"),
```

**Step 2: `src/value/repr/constructors.rs`** — Add constructor:

```rust
impl Value {
    pub fn my_type(data: MyTypeData) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::MyType(data))
    }
}
```

**Step 3: `src/value/repr/accessors.rs`** — Add predicate and accessor:

```rust
impl Value {
    pub fn is_my_type(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::MyType)
    }

    pub fn as_my_type(&self) -> Option<&MyTypeData> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() { return None; }
        match unsafe { deref(*self) } {
            HeapObject::MyType(data) => Some(data),
            _ => None,
        }
    }
}
```

**Step 4: `src/value/display.rs`** — Add arms in both `Display` and
`Debug` impls (search for existing heap type formatting like `LStringMut`):

```rust
// In Display impl, after @string handling:
if let Some(data) = self.as_my_type() {
    return write!(f, "<my-type:{}>", data);
}

// Debug impl delegates to Display for most heap types.
```

**Step 5: `src/value/repr/traits.rs`** — Add `PartialEq` arm:

```rust
// In the (self_obj, other_obj) match:
(HeapObject::MyType(a), HeapObject::MyType(b)) => a == b,
// or for reference equality:
(HeapObject::MyType(_), HeapObject::MyType(_)) => {
    std::ptr::eq(self_obj as *const _, other_obj as *const _)
}
```

**Step 6: `src/value/send.rs`** — Add to `SendValue`:

```rust
// If sendable, add a variant and implement from_value/into_value:
pub enum SendValue {
    // ...
    MyType(MyTypeOwnedData),
}

// In from_value():
HeapObject::MyType(data) => Ok(SendValue::MyType(data.clone())),

// In into_value():
SendValue::MyType(data) => alloc(HeapObject::MyType(data)),

// If NOT sendable:
HeapObject::MyType(_) => Err("Cannot send my-type".to_string()),
```

### After adding the type

You'll likely want primitives to create and manipulate it (see Recipe 1)
and a type-check predicate in `src/primitives/types.rs`.

---

---

## See also

- [Cookbook index](index.md)
