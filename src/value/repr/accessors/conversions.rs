use super::*;

/// Read-only handle on a mutable container's `RefCell`: exposes `borrow()`
/// and nothing else. The public face of the mutable-store seam
/// (docs/impl/region/rules.md Rule 5, mutable store) — code outside `value/` can read
/// an @array/@struct/@set/box/capture cell through this, but storing a
/// `Value` is only possible through the tracked funnels in `value/arena.rs`.
pub struct ReadCell<'a, T>(&'a std::cell::RefCell<T>);

impl<'a, T> ReadCell<'a, T> {
    /// Immutably borrow the contents (same panic-on-conflict semantics as
    /// `RefCell::borrow`).
    #[inline]
    pub fn borrow(&self) -> std::cell::Ref<'a, T> {
        self.0.borrow()
    }
}

impl Value {
    /// Extract as boolean if this is a bool.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self.tag {
            TAG_TRUE => Some(true),
            TAG_FALSE => Some(false),
            _ => None,
        }
    }
    /// Extract as integer if this is an int.
    #[inline]
    pub fn as_int(&self) -> Option<i64> {
        if self.is_int() {
            Some(self.payload as i64)
        } else {
            None
        }
    }
    /// Extract as float if this is a float.
    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        if self.is_float() {
            Some(f64::from_bits(self.payload))
        } else {
            None
        }
    }
    /// Extract as number (float), coercing integers.
    #[inline]
    pub fn as_number(&self) -> Option<f64> {
        if let Some(i) = self.as_int() {
            Some(i as f64)
        } else {
            self.as_float()
        }
    }
    /// Extract symbol ID if this is a symbol.
    #[inline]
    pub fn as_symbol(&self) -> Option<u32> {
        if self.is_symbol() {
            Some(self.payload as u32)
        } else {
            None
        }
    }
    /// Extract raw C pointer address if this is a pointer.
    #[inline]
    pub fn as_pointer(&self) -> Option<usize> {
        if self.is_pointer() {
            Some(self.payload as usize)
        } else {
            None
        }
    }
    /// Extract keyword name if this is a keyword.
    /// Acquires RwLock read lock and allocates a String.
    /// Use `keyword_hash()` when only comparing, not displaying.
    #[inline]
    pub fn as_keyword_name(&self) -> Option<String> {
        if self.is_keyword() {
            crate::value::keyword::keyword_name(self.payload)
        } else {
            None
        }
    }
    /// Extract heap pointer if this is a heap value.
    #[inline]
    pub fn as_heap_ptr(&self) -> Option<*const ()> {
        if self.is_heap() {
            Some(self.payload as *const ())
        } else {
            None
        }
    }
    /// Extract as pair if this is a pair cell.
    #[inline]
    pub fn as_pair(&self) -> Option<&crate::value::heap::Pair> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_pair() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Pair(c) => Some(c),
            _ => None,
        }
    }
    /// Read-only view of a mutable array — `borrow()` only. The mutation
    /// channel is the `value/`-scoped raw-cell accessor (`as_array_mut_raw`);
    /// every `Value` store flows through the tracked funnels in
    /// `value/arena.rs` (docs/impl/region/rules.md Rule 5, mutable store).
    #[inline]
    pub fn as_array_mut(&self) -> Option<ReadCell<'_, Vec<Value>>> {
        self.as_array_mut_raw().map(ReadCell)
    }
    /// Raw cell of a mutable array — the mutation channel (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_array_mut_raw(&self) -> Option<&std::cell::RefCell<Vec<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_array_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            // Deref the Rc to expose the inner RefCell directly. Callers
            // already borrow from the RefCell; the Rc is an implementation
            // detail that enables cross-fiber sharing (see heap.rs).
            HeapObject::LArrayMut { data, .. } => Some(&**data),
            _ => None,
        }
    }
    /// Read-only view of an @struct — `borrow()` only (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub fn as_struct_mut(
        &self,
    ) -> Option<ReadCell<'_, std::collections::BTreeMap<crate::value::heap::TableKey, Value>>> {
        self.as_struct_mut_raw().map(ReadCell)
    }
    /// Raw cell of an @struct — the mutation channel (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_struct_mut_raw(
        &self,
    ) -> Option<&std::cell::RefCell<std::collections::BTreeMap<crate::value::heap::TableKey, Value>>>
    {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_struct_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStructMut { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as struct if this is a struct.
    /// Returns a sorted slice of (key, value) pairs.
    #[inline]
    pub fn as_struct(&self) -> Option<&[(crate::value::heap::TableKey, Value)]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_struct() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStruct { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as closure if this is a closure.
    ///
    /// Returns a borrow of the arena-resident `Closure`. If you need an
    /// owned `Rc<Closure>` (e.g. for storing in a `Fiber` or `Frame`),
    /// clone explicitly: `Rc::new(value.as_closure().unwrap().clone())`.
    /// `Closure::clone` is O(1) — every non-Copy field is `Rc`-shared.
    #[inline]
    pub fn as_closure(&self) -> Option<&crate::value::heap::Closure> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_closure() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Closure { closure, .. } => Some(closure),
            _ => None,
        }
    }
    /// Read-only view of a user box — `borrow()` only (see
    /// [`Value::as_array_mut`]); [`Value::lbox_get`] copies the value out.
    #[inline]
    pub fn as_lbox(&self) -> Option<ReadCell<'_, Value>> {
        self.as_lbox_raw().map(ReadCell)
    }
    /// Raw cell of a user box — the mutation channel (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_lbox_raw(&self) -> Option<&std::cell::RefCell<Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_lbox() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBox { cell, .. } => Some(cell),
            _ => None,
        }
    }
    /// Read-only view of a compiler capture cell — `borrow()` only (see
    /// [`Value::as_array_mut`]); [`Value::capture_cell_get`] copies the
    /// value out.
    #[inline]
    pub fn as_capture_cell(&self) -> Option<ReadCell<'_, Value>> {
        self.as_capture_cell_raw().map(ReadCell)
    }
    /// Raw cell of a compiler capture cell — the mutation channel (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_capture_cell_raw(&self) -> Option<&std::cell::RefCell<Value>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_capture_cell() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::CaptureCell { cell, .. } => Some(cell),
            _ => None,
        }
    }
    /// Read-only view of a box or capture cell — `borrow()` only (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub fn as_box_or_capture(&self) -> Option<ReadCell<'_, Value>> {
        self.as_box_or_capture_raw().map(ReadCell)
    }
    /// Raw cell of either a user box or a capture cell — the mutation
    /// channel (see [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_box_or_capture_raw(&self) -> Option<&std::cell::RefCell<Value>> {
        self.as_lbox_raw().or_else(|| self.as_capture_cell_raw())
    }

    // ── Read-only views of the mutable containers ───────────────────
    //
    // The seam's public half (docs/impl/region/rules.md Rule 5, mutable store): borrow
    // guards and copy-outs that cannot store. Same borrow semantics as the
    // raw cells (panic on a conflicting mutable borrow).

    /// Borrow a mutable array's elements, read-only.
    #[inline]
    pub fn array_mut_ref(&self) -> Option<std::cell::Ref<'_, Vec<Value>>> {
        self.as_array_mut_raw().map(|c| c.borrow())
    }
    /// Borrow an @struct's entries, read-only.
    #[inline]
    pub fn struct_mut_ref(
        &self,
    ) -> Option<std::cell::Ref<'_, std::collections::BTreeMap<crate::value::heap::TableKey, Value>>>
    {
        self.as_struct_mut_raw().map(|c| c.borrow())
    }
    /// Borrow a mutable set's elements, read-only.
    #[inline]
    pub fn set_mut_ref(&self) -> Option<std::cell::Ref<'_, std::collections::BTreeSet<Value>>> {
        self.as_set_mut_raw().map(|c| c.borrow())
    }
    /// The value currently in a user box (`Value` is `Copy`).
    #[inline]
    pub fn lbox_get(&self) -> Option<Value> {
        self.as_lbox_raw().map(|c| *c.borrow())
    }
    /// The value currently in a compiler capture cell (`Value` is `Copy`).
    #[inline]
    pub fn capture_cell_get(&self) -> Option<Value> {
        self.as_capture_cell_raw().map(|c| *c.borrow())
    }
    /// The value currently in a box or capture cell (`Value` is `Copy`).
    #[inline]
    pub fn box_or_capture_get(&self) -> Option<Value> {
        self.as_box_or_capture_raw().map(|c| *c.borrow())
    }
    /// Extract the primitive definition if this is a native function.
    #[inline]
    pub fn as_native_def(&self) -> Option<&'static crate::primitives::def::PrimitiveDef> {
        if !self.is_native_fn() {
            return None;
        }
        // Immediate: the payload is the prim_id — the def's dense id in the
        // primitive registry. No deref (a native-fn has no heap cell).
        crate::primitives::prim_def(self.payload as u32)
    }
    /// Extract the bare function pointer if this is a native function.
    #[inline]
    pub fn as_native_fn(&self) -> Option<crate::value::heap::PrimFn> {
        self.as_native_def().map(|def| def.func)
    }
    /// Extract as array (immutable) if this is one.
    #[inline]
    pub fn as_array(&self) -> Option<&[Value]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_array() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LArray { elements, .. } => Some(elements),
            _ => None,
        }
    }
    /// Extract as set if this is a set.
    /// Returns a sorted slice of values (binary search for membership).
    #[inline]
    pub fn as_set(&self) -> Option<&[Value]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_set() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LSet { data, .. } => Some(data.as_slice()),
            _ => None,
        }
    }
    /// Read-only view of a mutable set — `borrow()` only (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub fn as_set_mut(&self) -> Option<ReadCell<'_, std::collections::BTreeSet<Value>>> {
        self.as_set_mut_raw().map(ReadCell)
    }
    /// Raw cell of a mutable set — the mutation channel (see
    /// [`Value::as_array_mut`]).
    #[inline]
    pub(in crate::value) fn as_set_mut_raw(
        &self,
    ) -> Option<&std::cell::RefCell<std::collections::BTreeSet<Value>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_set_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LSetMut { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as @string if this is an @string.
    #[inline]
    pub fn as_string_mut(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_string_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LStringMut { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as bytes if this is a bytes value.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_bytes() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBytes { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as @bytes if this is an @bytes value.
    #[inline]
    pub fn as_bytes_mut(&self) -> Option<&std::cell::RefCell<Vec<u8>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_bytes_mut() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LBytesMut { data, .. } => Some(data),
            _ => None,
        }
    }
    /// Extract as thread handle if this is a thread handle.
    #[inline]
    pub fn as_thread_handle(&self) -> Option<&crate::value::heap::ThreadHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_thread() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ThreadHandle { handle, .. } => Some(handle),
            _ => None,
        }
    }
    /// Extract as fiber handle if this is a fiber.
    #[inline]
    pub fn as_fiber(&self) -> Option<&crate::value::fiber::FiberHandle> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_fiber() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Fiber { handle, .. } => Some(handle),
            _ => None,
        }
    }
    /// Extract as syntax if this is a syntax object.
    #[inline]
    pub fn as_syntax(&self) -> Option<&crate::syntax::Syntax> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_syntax() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Syntax { syntax, .. } => Some(syntax),
            _ => None,
        }
    }
    /// Extract parameter (id, default) if this is a parameter.
    #[inline]
    pub fn as_parameter(&self) -> Option<(u32, Value)> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_parameter() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::Parameter { id, default, .. } => Some((*id, *default)),
            _ => None,
        }
    }
    /// Extract as FFI signature if this is an FFI signature.
    #[inline]
    pub fn as_ffi_signature(&self) -> Option<&crate::ffi::types::Signature> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_ffi_sig() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFISignature(sig, ..) => Some(sig),
            _ => None,
        }
    }
    /// Extract as FFI type descriptor if this is an FFI type.
    #[inline]
    pub fn as_ffi_type(&self) -> Option<&crate::ffi::types::TypeDesc> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_ffi_type() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::FFIType(desc) => Some(desc),
            _ => None,
        }
    }
    /// Extract as library handle ID if this is a library handle.
    #[inline]
    pub fn as_lib_handle(&self) -> Option<u32> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_lib_handle() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::LibHandle(id) => Some(*id),
            _ => None,
        }
    }
    /// Extract the managed pointer cell, if this is a managed pointer.
    #[inline]
    pub fn as_managed_pointer(&self) -> Option<&std::cell::Cell<Option<usize>>> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_managed_pointer() {
            return None;
        }
        match unsafe { deref(*self) } {
            HeapObject::ManagedPointer { addr, .. } => Some(addr),
            _ => None,
        }
    }
}
