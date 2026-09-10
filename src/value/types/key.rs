// audited: 2026-09-08
//! Struct keys: the form a key takes, and how keys compare, hash, and print.
//!
//! docs/impl/values.md
//! docs/impl/symbol.md
//!
//! A key owns no Rust-heap memory, so a struct's entries are page bytes. A
//! key built by `from_value` **borrows** what it read, which is right for a
//! probe and wrong to store; `intern_into` builds the stored form. Identity
//! is content for a string or an array and a name hash for a symbol or a
//! keyword, so a sorted struct probes correctly in every instance.

use std::fmt;

use crate::hir::region::RuntimeRegion;
use crate::value::heap::HeapTag;
use crate::value::Value;

use super::SymbolId;

/// A struct key: any immutable, non-float value, in a form that owns no Rust
/// heap memory. `Copy`, because the string, array, and heap arms hold a
/// `Value` pointing into a region rather than an allocation of their own —
/// which is what lets a struct's entries be page bytes.
///
/// A key from [`TableKey::from_value`] **borrows** what it read. That is right
/// for a probe and wrong to store; [`TableKey::intern_into`] builds the stored
/// form, copying a string or array payload into the destination region. See
/// [docs/impl/values.md](../../docs/impl/values.md) § "Struct keys".
#[derive(Clone, Copy)]
pub enum TableKey {
    Nil,
    Bool(bool),
    Int(i64),
    Symbol(SymbolId),
    /// An immutable string key, held as its `LString` value. Compared and
    /// hashed by content, so keys spelled alike match wherever their bytes
    /// live. A mutable `@string` is refused: it could change after insertion.
    String(Value),
    /// A keyword key is its 64-bit name hash — the keyword value's payload,
    /// exactly as `Symbol` is its `SymbolId`. Identity and order are pure
    /// functions of the hash, so a probe builds a key with no lock and no
    /// allocation, and a sorted struct probes correctly in every instance.
    /// Display resolves the spelling through the per-instance memo
    /// (docs/impl/symbol.md § "The display memo").
    Keyword(u64),
    EmptyList,
    /// An immutable array key, held as its `LArray` value. Every element is
    /// itself a valid key (`from_value` validates the whole tree), and the
    /// elements compare and hash as KEYS rather than as values, so an array
    /// key ranks its elements in the same order the surrounding struct uses.
    /// Mutable arrays are refused — mutation after insertion would break the
    /// hash invariant.
    Array(Value),
    /// Any other non-scalar immutable heap value used as a struct key.
    ///
    /// Stores the original `Value` directly. `from_value()` recursively validates
    /// that all sub-elements are themselves valid keys (immutable, non-float) but
    /// always stores the original `*val`, not a reconstructed copy.
    ///
    /// `Hash`/`Eq`/`Ord` delegate to `Value`'s implementations, which give:
    /// - **Identity semantics** for fiber, closure, external (compared by pointer)
    /// - **Structural semantics** for cons, set, struct, bytes, empty list
    Heap(Value),
}

impl TableKey {
    /// Build a keyword key from its spelling. Identity only — the spelling is
    /// not recorded anywhere; display resolves through the per-instance memo.
    pub fn keyword(name: &str) -> TableKey {
        TableKey::Keyword(crate::value::keyword::keyword_hash(name))
    }

    /// The spelling of a string key, borrowed from its region pages. `None`
    /// for every other variant — the one read site for a string key's bytes.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TableKey::String(v) => v.as_str(),
            _ => None,
        }
    }

    /// Build a **probe** key over `val`. The key borrows what it reads and
    /// allocates nothing, so a lookup costs no allocation. Storing the result
    /// needs [`TableKey::intern_into`] first.
    ///
    /// Returns `None` if the value cannot be used as a key.
    /// Callers produce their own error messages from the `None` case.
    pub fn from_value(val: &Value) -> Option<TableKey> {
        if val.is_nil() {
            Some(TableKey::Nil)
        } else if let Some(b) = val.as_bool() {
            Some(TableKey::Bool(b))
        } else if let Some(i) = val.as_int() {
            Some(TableKey::Int(i))
        } else if let Some(id) = val.as_symbol() {
            Some(TableKey::Symbol(id))
        } else if let Some(hash) = val.keyword_hash() {
            Some(TableKey::Keyword(hash))
        } else if val.as_str().is_some() {
            Some(TableKey::String(*val))
        } else if let Some(arr) = val.as_array() {
            // Validate the whole tree here, once, so every later walk over an
            // array key may assume each element converts.
            for elem in arr {
                TableKey::from_value(elem)?;
            }
            Some(TableKey::Array(*val))
        } else if val.is_empty_list() {
            Some(TableKey::EmptyList)
        } else if val.is_pair() {
            let pair = val.as_pair().unwrap();
            Self::from_value(&pair.first)?;
            Self::from_value(&pair.rest)?;
            Some(TableKey::Heap(*val))
        } else if val.is_bytes() {
            Some(TableKey::Heap(*val))
        } else if val.is_set() {
            let set = val.as_set().unwrap();
            for elem in set {
                Self::from_value(elem)?;
            }
            Some(TableKey::Heap(*val))
        } else if val.is_struct() {
            let entries = val.as_struct().unwrap();
            for (_key, value) in entries {
                Self::from_value(value)?;
            }
            Some(TableKey::Heap(*val))
        } else if val.is_fiber() || val.is_closure() || val.heap_tag() == Some(HeapTag::External) {
            Some(TableKey::Heap(*val))
        } else {
            None
        }
    }

    /// Build the **stored** form of this key, owned by `region`: a string or
    /// array payload is copied there, so a struct's key bytes are part of the
    /// struct's own body and the struct pins no region but its own. A key
    /// already resident in `region` is returned unchanged, and a `Heap` key is
    /// never copied — its identity is the point (docs/impl/values.md
    /// § "Struct keys").
    pub fn intern_into(&self, heap: &mut crate::value::FiberHeap, region: RuntimeRegion) -> Self {
        match self {
            TableKey::String(v) => {
                if crate::value::arena::region_of(heap, *v) == Some(region) {
                    return *self;
                }
                let text = v.as_str().expect("a string key holds a string");
                TableKey::String(crate::value::build::string(heap, text, region))
            }
            TableKey::Array(v) => {
                if crate::value::arena::region_of(heap, *v) == Some(region) {
                    return *self;
                }
                // Rebuild the spine in `region` with each element interned, so
                // an array key copies exactly what a string key does and
                // aliases exactly what a `Heap` element does.
                let elements: Vec<Value> = v
                    .as_array()
                    .expect("an array key holds an array")
                    .to_vec()
                    .iter()
                    .map(|e| {
                        TableKey::from_value(e)
                            .expect("from_value validated every element")
                            .intern_into(heap, region)
                            .to_value()
                    })
                    .collect();
                TableKey::Array(crate::value::build::array(heap, elements, region))
            }
            _ => *self,
        }
    }

    /// The `Value` this key is. The inverse of `from_value()`, and
    /// allocation-free: a scalar key is an immediate and every other key
    /// already holds its value. A stored key's value belongs to the struct's
    /// region, so a native handing one to its caller pays the ordinary
    /// pass-through retain (`arena::pass_through_retain`).
    pub fn to_value(&self) -> Value {
        match self {
            TableKey::Nil => Value::NIL,
            TableKey::Bool(b) => Value::bool(*b),
            TableKey::Int(i) => Value::int(*i),
            TableKey::Symbol(sid) => Value::symbol(*sid),
            TableKey::Keyword(hash) => Value::keyword_from_hash(*hash),
            TableKey::EmptyList => Value::EMPTY_LIST,
            TableKey::String(v) | TableKey::Array(v) | TableKey::Heap(v) => *v,
        }
    }

    /// Visit every heap `Value` this key holds — the cross-region references a
    /// struct key contributes to its owning struct's region.
    ///
    /// A string, array, or heap key holds a `Value` pointing into the region
    /// the key was interned into or built from; the scalar keys
    /// (nil/bool/int/symbol/keyword/empty-list) are immediates and carry no
    /// region reference. The region scan (`find_object_cross_refs`) walks
    /// these so a struct increfs and records the edge to each key's region at
    /// alloc, balanced by the free-time cascade — the same accounting struct
    /// VALUES get. An array key needs no recursion: the array object holds its
    /// own elements' edges, exactly as a set or struct key does.
    pub fn for_each_heap_value(&self, f: &mut impl FnMut(&Value)) {
        if let Some(v) = self.heap_value() {
            f(v);
        }
    }

    /// The `Value` this key holds, for the three arms that hold one. The
    /// scalar keys (nil/bool/int/symbol/keyword/empty-list) are immediates
    /// and hold none. This is the one place that says which arms those are;
    /// [`for_each_heap_value`](Self::for_each_heap_value) and
    /// [`with_heap_value`](Self::with_heap_value) both read it.
    pub fn heap_value(&self) -> Option<&Value> {
        match self {
            TableKey::String(v) | TableKey::Array(v) | TableKey::Heap(v) => Some(v),
            TableKey::Nil
            | TableKey::Bool(_)
            | TableKey::Int(_)
            | TableKey::Symbol(_)
            | TableKey::Keyword(_)
            | TableKey::EmptyList => None,
        }
    }

    /// This key with `v` in place of the `Value` it holds, and unchanged when
    /// it holds none. A copying walk uses it to rebuild a key around its
    /// copied value without re-deciding which arm the key is.
    pub fn with_heap_value(&self, v: Value) -> TableKey {
        match self {
            TableKey::String(_) => TableKey::String(v),
            TableKey::Array(_) => TableKey::Array(v),
            TableKey::Heap(_) => TableKey::Heap(v),
            _ => *self,
        }
    }

    /// Whether this key can cross a thread boundary. The answer is whether
    /// [`SendKey`](crate::value::send::SendKey) has a form for it, and it is
    /// asked there so the two cannot drift.
    pub fn is_sendable(&self) -> bool {
        crate::value::send::SendKey::from_key(self).is_some()
    }

    /// Compare the elements of two array keys AS KEYS, so an array key ranks
    /// its elements in the order the surrounding struct uses rather than in
    /// `Value`'s order (which ranks keywords before strings).
    fn cmp_elements(a: &Value, b: &Value) -> std::cmp::Ordering {
        let (a, b) = (
            a.as_array().expect("an array key holds an array"),
            b.as_array().expect("an array key holds an array"),
        );
        a.iter()
            .map(|e| TableKey::from_value(e).expect("from_value validated every element"))
            .cmp(
                b.iter()
                    .map(|e| TableKey::from_value(e).expect("from_value validated every element")),
            )
    }

    /// The rank a key's variant sorts at, which is its declaration order.
    /// `pub(super)` so the ordering test can read it: what it pins is that
    /// the ranks and the declaration order have not drifted apart.
    pub(super) fn discriminant_index(&self) -> u8 {
        match self {
            TableKey::Nil => 0,
            TableKey::Bool(_) => 1,
            TableKey::Int(_) => 2,
            TableKey::Symbol(_) => 3,
            TableKey::String(_) => 4,
            TableKey::Keyword(_) => 5,
            TableKey::EmptyList => 6,
            TableKey::Array(_) => 7,
            TableKey::Heap(_) => 8,
        }
    }
}

impl std::hash::Hash for TableKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            TableKey::Nil => {}
            TableKey::EmptyList => {}
            TableKey::Bool(b) => b.hash(state),
            TableKey::Int(i) => i.hash(state),
            TableKey::Symbol(id) => id.hash(state),
            // By content, not by pointer: two keys spelled alike must land in
            // the same bucket wherever their bytes live.
            TableKey::String(v) => v.as_str().expect("a string key holds a string").hash(state),
            TableKey::Keyword(s) => s.hash(state),
            TableKey::Array(v) => {
                let elements = v.as_array().expect("an array key holds an array");
                // Length first, as `Vec`'s own `Hash` does: without it
                // `[[1] [2]]` and `[[1 2]]` hash alike.
                elements.len().hash(state);
                for elem in elements {
                    TableKey::from_value(elem)
                        .expect("from_value validated every element")
                        .hash(state);
                }
            }
            // Delegate to Value's Hash. For Fiber/ThreadHandle/External
            // that hashes the backing Rc/Arc rather than the slot pointer,
            // so a `with-traits` wrapper is the same map key as the value
            // it wraps (see `repr/eq.rs`, "Wrapper variants take their
            // identity from the handle"). For cons/set/struct/bytes/
            // empty-list, gives structural hashing based on the content.
            TableKey::Heap(v) => v.hash(state),
        }
    }
}

impl PartialEq for TableKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (TableKey::Nil, TableKey::Nil) => true,
            (TableKey::Bool(a), TableKey::Bool(b)) => a == b,
            (TableKey::Int(a), TableKey::Int(b)) => a == b,
            (TableKey::Symbol(a), TableKey::Symbol(b)) => a == b,
            (TableKey::String(a), TableKey::String(b)) => a.as_str() == b.as_str(),
            (TableKey::Keyword(a), TableKey::Keyword(b)) => a == b,
            (TableKey::EmptyList, TableKey::EmptyList) => true,
            (TableKey::Array(a), TableKey::Array(b)) => {
                TableKey::cmp_elements(a, b) == std::cmp::Ordering::Equal
            }
            // Delegate to Value's PartialEq (stable identity for Fiber
            // and friends — see Hash impl above). Structural equality
            // for cons/set/struct/bytes/empty-list.
            (TableKey::Heap(a), TableKey::Heap(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for TableKey {}

impl PartialOrd for TableKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TableKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Variant ordering follows enum declaration order (same as derive).
        // Discriminant index: Nil=0, Bool=1, Int=2, Symbol=3, String=4, Keyword=5, EmptyList=6, Array=7, Heap=8
        let self_disc = self.discriminant_index();
        let other_disc = other.discriminant_index();
        match self_disc.cmp(&other_disc) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match (self, other) {
            (TableKey::Nil, TableKey::Nil) => std::cmp::Ordering::Equal,
            (TableKey::Bool(a), TableKey::Bool(b)) => a.cmp(b),
            (TableKey::Int(a), TableKey::Int(b)) => a.cmp(b),
            (TableKey::Symbol(a), TableKey::Symbol(b)) => a.cmp(b),
            (TableKey::String(a), TableKey::String(b)) => a.as_str().cmp(&b.as_str()),
            (TableKey::Keyword(a), TableKey::Keyword(b)) => a.cmp(b),
            (TableKey::EmptyList, TableKey::EmptyList) => std::cmp::Ordering::Equal,
            (TableKey::Array(a), TableKey::Array(b)) => TableKey::cmp_elements(a, b),
            // Delegate to Value's Ord. Stable identity for Fiber and
            // friends — see Hash impl above. Structural ordering for
            // cons/set/struct/bytes/empty-list.
            (TableKey::Heap(a), TableKey::Heap(b)) => a.cmp(b),
            _ => unreachable!("discriminant match already handled"),
        }
    }
}

/// Render a `TableKey` — the shared body for `Display` (`debug == false`) and
/// `Debug` (`debug == true`). A symbol or keyword key is a name hash, so its
/// spelling comes from the threaded `symbols` memo; without one it renders
/// unresolved (docs/impl/symbol.md § "Reading a name, and not reading one"). The
/// two modes diverge only in the symbol arm (Display prints the raw `SymbolId`;
/// Debug prints the quoted name) and in the nested recursion of array/heap keys,
/// which follows the outer mode.
pub(crate) fn fmt_table_key(
    key: &TableKey,
    symbols: Option<&crate::symbol::SymbolTable>,
    debug: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match key {
        TableKey::Nil => write!(f, "nil"),
        TableKey::Bool(b) => write!(f, "{}", b),
        TableKey::Int(i) => write!(f, "{}", i),
        TableKey::Symbol(id) => {
            if debug {
                match symbols.and_then(|s| s.name(*id)) {
                    Some(name) => write!(f, "'{}", name),
                    None => write!(f, "'#<symbol:{:#x}>", id.0),
                }
            } else {
                write!(f, "{:?}", id)
            }
        }
        // A formatter never panics on a broken invariant; it shows one. Every
        // constructor of a string key passes a string, so the fallback is a
        // marker a reader can act on rather than an empty pair of quotes.
        TableKey::String(v) => match v.as_str() {
            Some(s) => write!(f, "\"{}\"", s),
            None => write!(f, "#<string-key:{:#x}>", v.payload),
        },
        TableKey::Keyword(hash) => {
            match crate::value::keyword::resolve_keyword_name(symbols, *hash) {
                Some(name) => write!(f, ":{}", name),
                None => write!(f, "#<keyword:{:#x}>", hash),
            }
        }
        TableKey::EmptyList => write!(f, "()"),
        TableKey::Array(v) => {
            let Some(elements) = v.as_array() else {
                return write!(f, "#<array-key:{:#x}>", v.payload);
            };
            write!(f, "[")?;
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                match TableKey::from_value(elem) {
                    Some(k) => fmt_table_key(&k, symbols, debug, f)?,
                    None => crate::value::display::fmt_value(elem, symbols, debug, f)?,
                }
            }
            write!(f, "]")
        }
        TableKey::Heap(v) => crate::value::display::fmt_value(v, symbols, debug, f),
    }
}

impl fmt::Display for TableKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self, None, false, f)
    }
}

/// A `TableKey` paired with a memo for name-resolving Debug-style rendering —
/// the key analogue of `Value::debug_with`, for formatters that print keys
/// outside `fmt_value`'s recursion.
pub(crate) struct TableKeyDisplay<'a>(pub &'a TableKey, pub Option<&'a crate::symbol::SymbolTable>);

impl fmt::Display for TableKeyDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self.0, self.1, true, f)
    }
}

impl fmt::Debug for TableKey {
    /// Machine-readable representation of table keys.
    /// Symbols: `'#<symbol:hash>` — a bare `Debug` threads no memo, so it has no
    /// name to print; `fmt_table_key` with a memo renders `'name`.
    /// Strings: "value" (with quotes)
    /// Keywords: :name
    /// Others: same as Display
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_table_key(self, None, true, f)
    }
}
