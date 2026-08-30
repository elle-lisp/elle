//! Bincode serde for [`SendValue`] and [`TableKey`], used by the stdlib disk
//! cache (`crate::compiler::stdlib_cache`).
//!
//! `SendValue` is the owned deep-copy form the send module produces for
//! cross-thread transport, and it is exactly the shape a disk format needs:
//! no `Rc`, no pointers, symbols by name. Serde is implemented for it
//! directly rather than for `Value`, which carries process-local
//! ids/pointers.
//!
//! Only the pure-data variants are supported; runtime-resource variants
//! (channels, ports, FFI descriptors) return an error, which makes the cache
//! miss and the caller recompile — safe, never wrong.
//!
//! Both directions of each impl go through one derived mirror enum so the
//! bincode encoding cannot drift: a hand-written `Serialize` that emits
//! `(u8, payload)` tuples does not round-trip against a derived
//! `Deserialize`, which reads bincode's enum tag
//! (`tablekey_map_roundtrips_through_bincode` pins this).

use super::SendValue;
use super::SendableClosure;
use crate::value::{TableKey, Value};

/// Symmetric serde mirror for `SendValue`. Both directions go through this
/// enum so the bincode encoding is identical (a hand-written `Serialize` that
/// emitted tuples would not round-trip against a derived `Deserialize`, which
/// expects bincode's enum encoding).
/// Which sequence a `Mirror::Seq` came from. Eight `SendValue` variants share
/// three shapes — a sequence, a map, a byte run — and the shape alone does not
/// say which. Derived, not hand-written, so both directions read one encoding.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
enum SeqKind {
    Array,
    Tuple,
    LSet,
    LSetMut,
}

/// Which map a `Mirror::Map` came from: mutability is the difference.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
enum MapKind {
    Struct,
    StructMut,
}

/// Which byte run a `Mirror::Buffer` came from. `Buffer` is a mutable
/// `@string` and `Bytes` an immutable one, so dropping the tag turns a mutable
/// value immutable on reload.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
enum BytesKind {
    Buffer,
    Bytes,
    Blob,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum Mirror {
    Immediate(Value),
    Keyword(String),
    Symbol {
        name: String,
        id: u32,
    },
    String(String),
    Pair(Box<Mirror>, Box<Mirror>, Box<Mirror>),
    Seq(SeqKind, Vec<Mirror>, Box<Mirror>),
    Map(
        MapKind,
        std::collections::BTreeMap<TableKey, Mirror>,
        Box<Mirror>,
    ),
    Buffer(BytesKind, Vec<u8>, Box<Mirror>),
    Float(f64),
    Closure(Box<SendableClosure>),
    Ref(usize),
    CaptureCell(Box<Mirror>, Box<Mirror>),
}

impl serde::Serialize for SendValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use super::SendValue as SV;
        use serde::ser::Error;
        fn to_mirror(sv: &SV) -> Result<Mirror, String> {
            if let SV::Immediate(v) = sv {
                if v.is_heap() && !v.is_native_fn() {
                    return Err(format!("Immediate heap value {}", v.type_name()));
                }
            }
            Ok(match sv {
                SV::Immediate(v) => Mirror::Immediate(*v),
                SV::Keyword(k) => Mirror::Keyword(k.clone()),
                SV::Symbol { name, id } => Mirror::Symbol {
                    name: name.clone(),
                    id: *id,
                },
                SV::String(st) => Mirror::String(st.clone()),
                SV::Pair(a, b, t) => Mirror::Pair(
                    Box::new(to_mirror(a)?),
                    Box::new(to_mirror(b)?),
                    Box::new(to_mirror(t)?),
                ),
                SV::Array(v, t) | SV::Tuple(v, t) | SV::LSet(v, t) | SV::LSetMut(v, t) => {
                    let kind = match sv {
                        SV::Array(..) => SeqKind::Array,
                        SV::Tuple(..) => SeqKind::Tuple,
                        SV::LSet(..) => SeqKind::LSet,
                        _ => SeqKind::LSetMut,
                    };
                    Mirror::Seq(
                        kind,
                        v.iter().map(to_mirror).collect::<Result<_, _>>()?,
                        Box::new(to_mirror(t)?),
                    )
                }
                SV::Struct(m, t) | SV::StructMut(m, t) => Mirror::Map(
                    if matches!(sv, SV::Struct(..)) {
                        MapKind::Struct
                    } else {
                        MapKind::StructMut
                    },
                    m.iter()
                        .map(|(k, v)| Ok((k.clone(), to_mirror(v)?)))
                        .collect::<Result<_, String>>()?,
                    Box::new(to_mirror(t)?),
                ),
                SV::Buffer(v, t) | SV::Bytes(v, t) | SV::Blob(v, t) => {
                    let kind = match sv {
                        SV::Buffer(..) => BytesKind::Buffer,
                        SV::Bytes(..) => BytesKind::Bytes,
                        _ => BytesKind::Blob,
                    };
                    Mirror::Buffer(kind, v.clone(), Box::new(to_mirror(t)?))
                }
                SV::LBox(..) => return Err("LBox not needed by stdlib cache".into()),
                SV::CaptureCell(a, b) => {
                    Mirror::CaptureCell(Box::new(to_mirror(a)?), Box::new(to_mirror(b)?))
                }
                SV::Float(f) => Mirror::Float(*f),
                SV::Closure(c) => Mirror::Closure(c.clone()),
                SV::Ref(r) => Mirror::Ref(*r),
                _ => return Err("SendValue variant not serializable by stdlib cache".into()),
            })
        }
        to_mirror(self).map_err(Error::custom)?.serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for SendValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use super::SendValue as SV;
        fn from_mirror(m: Mirror) -> Result<SV, String> {
            Ok(match m {
                Mirror::Immediate(v) => SV::Immediate(v),
                Mirror::Keyword(k) => SV::Keyword(k),
                Mirror::Symbol { name, id } => SV::Symbol { name, id },
                Mirror::String(st) => SV::String(st),
                Mirror::Pair(a, b, t) => SV::Pair(
                    Box::new(from_mirror(*a)?),
                    Box::new(from_mirror(*b)?),
                    Box::new(from_mirror(*t)?),
                ),
                Mirror::Seq(kind, v, t) => {
                    let vals = v
                        .into_iter()
                        .map(from_mirror)
                        .collect::<Result<Vec<_>, _>>()?;
                    let traits = Box::new(from_mirror(*t)?);
                    match kind {
                        SeqKind::Array => SV::Array(vals, traits),
                        SeqKind::Tuple => SV::Tuple(vals, traits),
                        SeqKind::LSet => SV::LSet(vals, traits),
                        SeqKind::LSetMut => SV::LSetMut(vals, traits),
                    }
                }
                Mirror::Map(kind, m, t) => {
                    let map = m
                        .into_iter()
                        .map(|(k, v)| Ok((k, from_mirror(v)?)))
                        .collect::<Result<_, String>>()?;
                    let traits = Box::new(from_mirror(*t)?);
                    match kind {
                        MapKind::Struct => SV::Struct(map, traits),
                        MapKind::StructMut => SV::StructMut(map, traits),
                    }
                }
                Mirror::Buffer(kind, v, t) => {
                    let traits = Box::new(from_mirror(*t)?);
                    match kind {
                        BytesKind::Buffer => SV::Buffer(v, traits),
                        BytesKind::Bytes => SV::Bytes(v, traits),
                        BytesKind::Blob => SV::Blob(v, traits),
                    }
                }
                Mirror::Float(f) => SV::Float(f),
                Mirror::Closure(c) => SV::Closure(c),
                Mirror::Ref(r) => SV::Ref(r),
                Mirror::CaptureCell(a, b) => {
                    SV::CaptureCell(Box::new(from_mirror(*a)?), Box::new(from_mirror(*b)?))
                }
            })
        }
        from_mirror(Mirror::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Symmetric serde mirror for `TableKey`, for the same reason as [`Mirror`]:
/// both directions must share one bincode encoding. `Symbol` has no variant —
/// a symbol key carries only a process-local id, no name the loader could
/// re-intern, so it is rejected at serialize time (a cache miss, never a key
/// silently bound to an arbitrary symbol). `Heap` likewise.
#[derive(serde::Serialize, serde::Deserialize)]
enum KeyMirror {
    Nil,
    Bool(bool),
    Int(i64),
    String(String),
    Keyword(String),
    EmptyList,
    Array(Vec<KeyMirror>),
}

impl serde::Serialize for TableKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use crate::value::TableKey as TK;
        use serde::ser::Error;
        fn to_mirror(k: &TK) -> Result<KeyMirror, String> {
            Ok(match k {
                TK::Nil => KeyMirror::Nil,
                TK::Bool(b) => KeyMirror::Bool(*b),
                TK::Int(i) => KeyMirror::Int(*i),
                TK::String(st) => KeyMirror::String(st.clone()),
                TK::Keyword(kw) => KeyMirror::Keyword(kw.clone()),
                TK::EmptyList => KeyMirror::EmptyList,
                TK::Array(a) => {
                    KeyMirror::Array(a.iter().map(to_mirror).collect::<Result<_, _>>()?)
                }
                TK::Symbol(_) => {
                    return Err("TableKey::Symbol has no name to re-intern".into());
                }
                TK::Heap(_) => {
                    return Err("TableKey::Heap not serializable by stdlib cache".into());
                }
            })
        }
        to_mirror(self).map_err(Error::custom)?.serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for TableKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use crate::value::TableKey as TK;
        fn from_mirror(m: KeyMirror) -> TK {
            match m {
                KeyMirror::Nil => TK::Nil,
                KeyMirror::Bool(b) => TK::Bool(b),
                KeyMirror::Int(i) => TK::Int(i),
                KeyMirror::String(st) => TK::String(st),
                KeyMirror::Keyword(kw) => TK::Keyword(kw),
                KeyMirror::EmptyList => TK::EmptyList,
                KeyMirror::Array(a) => TK::Array(a.into_iter().map(from_mirror).collect()),
            }
        }
        Ok(from_mirror(KeyMirror::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A struct constant's `TableKey`s must survive bincode. The trap: a
    /// hand-written `Serialize` that emits `(u8, payload)` tuples against a
    /// derived `Deserialize` that reads bincode's enum tag decodes garbage —
    /// every struct-bearing cache entry would fail to load (and the failure
    /// is silent: the caller just recompiles, forever).
    #[test]
    fn tablekey_map_roundtrips_through_bincode() {
        use super::SendValue as SV;

        let mut map: BTreeMap<TableKey, SV> = BTreeMap::new();
        map.insert(TableKey::Int(7), SV::Immediate(Value::int(1)));
        map.insert(TableKey::String("s".into()), SV::Immediate(Value::int(2)));
        map.insert(TableKey::Keyword("k".into()), SV::Immediate(Value::int(3)));
        map.insert(
            TableKey::Array(vec![TableKey::Bool(true), TableKey::EmptyList]),
            SV::Immediate(Value::int(4)),
        );
        let sv = SV::Struct(map, Box::new(SV::Immediate(Value::NIL)));

        let bytes = bincode::serialize(&sv).expect("serializes");
        let back: SV = bincode::deserialize(&bytes).expect("deserializes");
        let SV::Struct(m, _) = back else {
            panic!("round-trip changed the container kind");
        };
        assert_eq!(m.len(), 4, "all keys survive");
        for (k, want) in [
            (TableKey::Int(7), 1),
            (TableKey::String("s".into()), 2),
            (TableKey::Keyword("k".into()), 3),
            (
                TableKey::Array(vec![TableKey::Bool(true), TableKey::EmptyList]),
                4,
            ),
        ] {
            let Some(SV::Immediate(v)) = m.get(&k) else {
                panic!("key {k:?} lost or value kind changed");
            };
            assert_eq!(v.as_int(), Some(want), "value under {k:?}");
        }
    }

    /// A symbol `TableKey` carries only a process-local id — no name travels
    /// with it, so a loader cannot re-intern it. Storing it raw would attach
    /// the entry to an arbitrary symbol in the loading process; the only safe
    /// treatment is a serialize error, which the cache layer turns into a
    /// plain miss.
    #[test]
    fn tablekey_symbol_key_is_rejected_not_mistranslated() {
        use super::SendValue as SV;
        use crate::value::SymbolId;

        let mut map: BTreeMap<TableKey, SV> = BTreeMap::new();
        map.insert(TableKey::Symbol(SymbolId(42)), SV::Immediate(Value::int(1)));
        let sv = SV::Struct(map, Box::new(SV::Immediate(Value::NIL)));
        assert!(
            bincode::serialize(&sv).is_err(),
            "symbol keys must fail to serialize, not carry a raw id"
        );
    }
}
