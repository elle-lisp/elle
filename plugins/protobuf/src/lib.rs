//! Elle protobuf plugin — dynamic protobuf encode/decode via descriptor pools.
//!
//! Provides:
//!   - `protobuf/schema` — parse `.proto` text into a descriptor pool
//!   - `protobuf/schema-bytes` — load binary `FileDescriptorSet` into a pool
//!   - `protobuf/encode` — encode an Elle struct to protobuf bytes
//!   - `protobuf/decode` — decode protobuf bytes to an Elle struct
//!   - `protobuf/messages` — list message names in a pool
//!   - `protobuf/fields` — list fields of a message
//!   - `protobuf/enums` — list enum types in a pool

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::types::Arity;
use elle::value::Value;

mod convert;
mod inspect;
mod schema;

// ---------------------------------------------------------------------------
// Primitive wrappers
// ---------------------------------------------------------------------------

fn prim_schema(args: &[Value]) -> (SignalBits, Value) {
    schema::prim_schema(args)
}

fn prim_schema_bytes(args: &[Value]) -> (SignalBits, Value) {
    schema::prim_schema_bytes(args)
}

fn prim_encode(args: &[Value]) -> (SignalBits, Value) {
    convert::encode(args)
}

fn prim_decode(args: &[Value]) -> (SignalBits, Value) {
    convert::decode(args)
}

fn prim_messages(args: &[Value]) -> (SignalBits, Value) {
    inspect::prim_messages(args)
}

fn prim_fields(args: &[Value]) -> (SignalBits, Value) {
    inspect::prim_fields(args)
}

fn prim_enums(args: &[Value]) -> (SignalBits, Value) {
    inspect::prim_enums(args)
}

// ---------------------------------------------------------------------------
// Primitive registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "protobuf/schema",
        func: prim_schema,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Parse .proto source text into a descriptor pool. Optional second arg: {:path \"name.proto\" :includes [\"dir\"]}.",
        params: &["proto-string", "opts?"],
        category: "protobuf",
        example: r#"(protobuf/schema "syntax = \"proto3\"; message Foo { string x = 1; }")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/schema-bytes",
        func: prim_schema_bytes,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Load a pre-compiled binary FileDescriptorSet into a descriptor pool.",
        params: &["fds-bytes"],
        category: "protobuf",
        example: "(protobuf/schema-bytes my-fds-bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/encode",
        func: prim_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Encode an Elle struct to protobuf bytes using the given descriptor pool and message name.",
        params: &["pool", "message-name", "value"],
        category: "protobuf",
        example: r#"(protobuf/encode pool "Person" {:name "Alice" :age 30})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/decode",
        func: prim_decode,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Decode protobuf bytes to an Elle struct using the given descriptor pool and message name.",
        params: &["pool", "message-name", "bytes"],
        category: "protobuf",
        example: r#"(protobuf/decode pool "Person" buf)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/messages",
        func: prim_messages,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "List fully-qualified message names in a descriptor pool.",
        params: &["pool"],
        category: "protobuf",
        example: "(protobuf/messages pool)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/fields",
        func: prim_fields,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "List fields of a message. Returns array of {:name :number :type :label} structs.",
        params: &["pool", "message-name"],
        category: "protobuf",
        example: r#"(protobuf/fields pool "Person")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "protobuf/enums",
        func: prim_enums,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "List enum types in a descriptor pool. Returns array of {:name :values} structs.",
        params: &["pool"],
        category: "protobuf",
        example: "(protobuf/enums pool)",
        aliases: &[],
    },
];

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
#[no_mangle]
elle::elle_plugin_init!(PRIMITIVES, "protobuf/");
