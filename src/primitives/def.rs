//! Primitive definition type for declarative registration.
//!
//! Each primitive module exports a `static PRIMITIVES: &[PrimitiveDef]`
//! table. `register_primitives` iterates all tables to register
//! primitives with the VM and build the metadata maps.

use crate::signals::Signal;
use crate::value::types::{Arity, PrimFn};
use crate::value::{SymbolId, Value};
use std::collections::HashMap;

/// Statically known return type of a primitive, consumed by type
/// inference (`hir::typeinfer`).
///
/// This is the single source of truth: inference reads it through
/// `registration::def_by_name` instead of keeping a parallel
/// name→type string table that silently drifts when primitives are
/// renamed, aliased, or added. `Unknown` (the default) means "no
/// static claim" — inference falls back to Top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetType {
    /// Not statically known (the default).
    Unknown,
    Int,
    Float,
    Bool,
    String,
    /// Mutable string (`@string`). Sound only for a primitive that *always*
    /// yields a mutable string (the `@string` constructor builds fresh; the
    /// `bytes`/`@bytes` lesson: a coercion that inherits the argument's
    /// mutability is NOT a clean `Mutable*`/immutable return type).
    MutableString,
    Keyword,
    Bytes,
    MutableBytes,
    Array,
    MutableArray,
    Struct,
    MutableStruct,
    Set,
    MutableSet,
    /// A fiber, on every normally-completing path (`fiber/new`). Beside type
    /// inference, the ownership forest reads this: a declared-`Fiber` call's
    /// result region joins `RegionInfo::fiber_result_regions` and is never a
    /// member of a region-rooted Owned subtree — a fiber acquires aliases by
    /// merely running (the scheduler's parent/child chain, `fiber/child`-style
    /// graph reads), so no structural obligation can bound its borrows
    /// (docs/impl/region/adopt.md § "The fiber member — refused at the class
    /// level"). A NULLABLE fiber result (`fiber/child` before any resume)
    /// declares `Unknown` instead, or the type-dispatch prune would cut a live
    /// nil arm.
    Fiber,
    /// Returns its first argument (mutating pass-throughs).
    FirstArg,
}

/// Declared region behavior of a primitive — the native-call analogue of
/// Rule 2's opaque-call exception and Rule 5's escape list
/// (docs/impl/region/effects.md "Native region effects: declared, not guessed").
///
/// The region solver keys the opaque-call arg clique on this:
/// `Immediate`/`Fresh`/`PassThrough` calls record no may-store edges
/// between their heap arguments; `Stores` records
/// directed edges from the listed arguments only; `Mixed` and `Unknown`
/// (the default) keep the full mutual clique — the conservative worst case
/// (over-keep, never mis-free). `Sends` and `Delivers` mark the listed args
/// as fiber-frontier crossings for the ownership forest (the **send** Shared
/// seed) and record no edges, because each seam counts its own reference at
/// runtime — see the variant docs.
///
/// A declaration is a soundness claim, so it is checked forever: in debug
/// builds `dispatch_native_call` compares the declared effect against
/// `region_of(result)` after every normally-completing native call (the
/// declaration oracle) and panics on a lie, naming the primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionEffect {
    /// The result is always an immediate (int, float, bool, nil,
    /// keyword); no argument is stored anywhere that outlives the call.
    Immediate,
    /// A heap result is freshly allocated in the call's own result
    /// region; no argument is stored anywhere *outside* the result. The
    /// result MAY embed references to the arguments (`pair`, `list`,
    /// struct constructors) — those are alloc-scan counted (Rule 5), so
    /// an embedding constructor is `Fresh`, not `Stores`. (An immediate
    /// result — e.g. a nil error-path return — is always permitted; the
    /// claim constrains the heap case. Same for the variants below.)
    Fresh,
    /// A heap result is one of the arguments or a value living in an
    /// argument's region (or an immediate); no argument is stored.
    /// The dispatch pass-through retain hands the caller its counted
    /// reference.
    PassThrough,
    /// The listed (0-based) arguments may be stored into another
    /// argument or an external structure without a runtime count the
    /// solver can rely on. A heap result is fresh. Stores into the
    /// result are `Fresh` (alloc-scan counted); a funnel-storing native
    /// declares `Funnel`.
    Stores { args: &'static [usize] },
    /// The listed arguments cross a **fiber boundary**: they are handed to
    /// another fiber (`chan/send`'s message rides the channel to the receiving
    /// fiber, by pointer under the single-threaded scheduler). The store is
    /// seam-counted, so the solver records NO edges: the send body retains the
    /// message's region at runtime after a successful enqueue
    /// (`EscapeSite::ChanSend` in `prim_chan_send`), and the receive lowers it
    /// (`release_received_message`). A compile-time edge cannot carry this
    /// reference — it is keyed on a region pair, and at a real call site the
    /// channel is typically an upvalue or module-level binding, so no pair
    /// exists and no incref would be emitted
    /// (tests/elle/region-chan-send-owned-param-uaf.lisp). The *escape* of the
    /// message is the escape analysis's fiber/send facet (`hir::escape`), the
    /// **send** half of the ownership forest's fiber-facet Shared seed. The
    /// distinction from `Stores` is the *frontier*: a `Stores` into a local
    /// aggregate or a callback (`ffi/callback`) is containment and stays an
    /// Owned-candidate; a `Sends` leaves the fiber and cannot be Owned. A heap
    /// result is fresh (`chan/send` returns a fresh `[:ok]`), so the
    /// declaration oracle's result-side check is identical to `Stores`.
    Sends { args: &'static [usize] },
    /// Every argument store goes through the mutable-store funnel
    /// (arena.rs `push_with_incref`-style, runtime-counted), and the
    /// result may be fresh OR pass-through (`put` copies an immutable
    /// struct, returns a mutable one). No solver edges — a compile-time
    /// clique incref would double-count the funnel's runtime incref
    /// against the container's single free-time cascade decref (the
    /// `put`/`push` store probes in `tests/elle/oracle.lisp` pin the
    /// seam reclaiming). No result-side
    /// oracle constraint, exactly as `Mixed`.
    Funnel,
    /// Examined, and confirmed to store NO argument (every argument is read
    /// or copied out — into a Rust `String`/`Vec`, the kernel, a fresh
    /// structure — never retained uncounted), but the result is non-fresh and
    /// non-pass-through: it lives in neither the call's own region nor an
    /// argument's (e.g. a value minted on the scheduler heap and delivered by
    /// a fiber resume — `subprocess/exec`'s process struct). `Mixed` minus the
    /// store: NO arg clique (nothing is stored, so the clique would only leak)
    /// and no result-side oracle constraint (the result may live anywhere).
    /// The clique is keyed on the *store*, not the result shape, so a no-store
    /// opaque-result native is `Opaque`, never `Mixed`
    /// (docs/impl/region/effects.md § Opaque).
    Opaque,
    /// The listed (0-based) arguments are DELIVERED to another fiber — installed
    /// into that fiber's signal slot — and the result is unbounded. The fiber
    /// value installers declare it: `fiber/resume`'s resume value,
    /// `fiber/abort`'s and `fiber/cancel`'s error payload, `fiber/emit`'s
    /// emitted value. Each carries both properties `Mixed` conflates, so this
    /// variant answers them separately:
    ///
    /// - **No arg clique** (`Funnel`'s answer). Every install seam accounts for
    ///   its own reference at runtime: an install that OUTLIVES the call takes
    ///   the park-retain and records the `fiber → signal` outgoing edge
    ///   (`record_terminal_signal_park`), and an install the next step CONSUMES
    ///   is a transient handover the caller's own parked frame keeps alive. A
    ///   compile-time incref would double-count the first against its single
    ///   free-time cascade decref and never balance the second.
    /// - **A fiber-frontier escape seed** for the listed args (`Sends`'s
    ///   answer): the value goes to a fiber this activation does not bound, so
    ///   it is never Owned by the installing subtree.
    /// - **An unbounded result** (`Opaque`'s answer): a resume hands back
    ///   whatever the resumed fiber yields, and an abort of a dead fiber hands
    ///   back a value read out of the fiber argument. No result-side oracle
    ///   check, and the walk records `result ⊒ each argument`.
    ///
    /// The distinction from `Sends` is who balances the seam's reference: a
    /// channel buffer is external to the region system and nothing cascades it,
    /// so `chan/send`'s seam retain IS the message's reference and the receive
    /// lowers it. A fiber's signal slot is a scanned field of a region-managed
    /// fiber object, so an outliving install is balanced by the fiber's
    /// free-time signal scan (docs/impl/region/effects.md § `Delivers`).
    Delivers { args: &'static [usize] },
    /// Examined, and the native stores arguments *uncounted* (the property
    /// the arg clique exists to cover) — and/or returns a result that is
    /// neither always-fresh nor always-pass-through. A positive declaration —
    /// "we read it; this is the honest worst case." A native that stores
    /// nothing but merely returns a non-fresh result is `Opaque`, not `Mixed`.
    Mixed,
    /// Nobody has looked (the default). Unexamined primitives, every
    /// plugin-supplied definition, and user-supplied functions.
    /// Operationally identical to `Mixed` (full arg clique, no oracle
    /// check); epistemically the declaration work queue.
    Unknown,
}

/// Declarative definition of a primitive function.
///
/// All metadata for a primitive lives here. Each primitive module
/// exports a static array of these. Adding a new metadata field
/// means adding it here with a default; existing tables use
/// `..PrimitiveDef::DEFAULT`.
pub struct PrimitiveDef {
    /// The Elle-facing name (e.g., "math/sin", "pair").
    pub name: &'static str,
    /// The Rust implementation.
    pub func: PrimFn,
    /// Signal (errors, yields, etc.).
    pub signal: Signal,
    /// Argument count constraint.
    pub arity: Arity,
    /// One-line description for help/hover/docs.
    pub doc: &'static str,
    /// Parameter names for signature help.
    /// Empty slice for nullary or variadic-only functions.
    pub params: &'static [&'static str],
    /// Module/category (e.g., "math", "string", "file").
    /// Empty string for core (unprefixed) primitives.
    pub category: &'static str,
    /// Runnable example in Elle syntax. Picked up by elle-doc.
    /// Empty string if no example.
    pub example: &'static str,
    /// Aliases — additional names that resolve to the same function.
    /// Registered with identical metadata.
    pub aliases: &'static [&'static str],
    /// Declared region behavior. See [`RegionEffect`]. The region solver
    /// keys the opaque-call arg clique on this, and the declaration
    /// oracle (debug builds) checks the result side after every native
    /// call.
    pub effect: RegionEffect,
    /// The 0-based argument indices this primitive EMBEDS into its fresh result —
    /// meaningful only with [`RegionEffect::Fresh`]. The result's region holds a
    /// reference to each listed argument's region (as `%pair` embeds its car/cdr), so
    /// the region walk records `result ⊇ arg` in `RegionInfo::containment_edges` for
    /// each: the compile-time analog of the runtime alloc-scan
    /// (`find_object_cross_refs`) that counts the same embedding at allocation. Without
    /// it the ownership forest cannot see a captured value flow OUT through an escaping
    /// result and would fold it into the capturing closure's Owned subtree
    /// (docs/impl/region/effects.md § "Native region effects"; region/adopt.md § "The
    /// funnel adopt" — the side-field embed analog). Empty (the default) for a `Fresh`
    /// native that embeds none of its arguments (`popn`), which is exactly why `Fresh`
    /// alone cannot carry the fact. `with-traits` is the canonical declarant (`&[1]` —
    /// its `traits` side-field embeds the arg-1 table into the cloned result).
    pub embeds: &'static [usize],
    /// Statically known return type, for type inference. See [`RetType`].
    pub ret: RetType,
    /// The native's heap result is an element MOVED OUT of a container argument
    /// (`%pop`/`pop` remove and return the last @array element), not shared with
    /// it (`first`/`get`) nor discarded (`del`/`remove`). A moved-out result needs
    /// the pass-through retain (the caller's owning reference), but it must be
    /// taken BEFORE the container releases its own — otherwise a sole-owned
    /// element's region is freed while the returned Value still points into it
    /// (the free-before-retain UAF the `raw-pop` oracle probe pins). The native
    /// body performs that retain itself (`arena::pop_with_decref`), so
    /// `dispatch_native_call` must SKIP its own `pass_through_retain` — applying it
    /// again double-counts (a per-op leak). Orthogonal to `effect` (`%pop` is
    /// `PassThrough`, `pop` is `Funnel`): the retain-ordering fact is not a
    /// store/result-shape claim, so it rides its own flag. Empty/false (the
    /// default) for every non-removing native.
    pub moves_out: bool,
    /// Every heap result of this native already carries the caller's one owing
    /// reference, so `dispatch_native_call` must SKIP its pass-through retain —
    /// applying it would hand the caller two references against one release,
    /// stranding the result's region graph per call. Declared by the
    /// re-entrant natives whose result is produced by running compiled code on
    /// the driving VM (`import`'s module body, the `compile/*-module` test
    /// loaders' setup accumulator — each via `run_thunk_to_completion`): the
    /// value leaves that code through the return convention, and its return
    /// mint IS the caller's reference. A declarant must uphold the claim on
    /// every normally-completing path — `import`'s plugin path returns a cached
    /// value no thunk minted, so it takes an explicit
    /// `EscapeSite::NativeCallResult` retain in the body. The `moves_out`
    /// sibling states the same "body already supplied the reference" fact for
    /// container removal; this flag states it for thunk-run production
    /// (docs/impl/region/effects.md § "Native region effects").
    /// Consumed only at dispatch — no solver site reads it (a thunk-run result
    /// is a call result like any other on the compile side). False (the
    /// default) for every native whose result the dispatch retain must fund.
    pub result_minted: bool,
}

impl PrimitiveDef {
    /// Default for struct-update syntax. Intentionally panics at
    /// runtime if `func` is called — forces explicit initialization.
    pub const DEFAULT: PrimitiveDef = PrimitiveDef {
        name: "",
        func: _default_prim,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "",
        params: &[],
        category: "",
        example: "",
        aliases: &[],
        effect: RegionEffect::Unknown,
        embeds: &[],
        ret: RetType::Unknown,
        moves_out: false,
        result_minted: false,
    };
}

/// Placeholder function for DEFAULT — should never be called.
const fn _default_prim(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[crate::value::Value],
) -> (crate::value::fiber::SignalBits, crate::value::Value) {
    panic!("PrimitiveDef::DEFAULT func called — this is a bug")
}

/// Declare primitive `PrimitiveDef`s. One entry is
/// `"elle-name" => rust_func { key: value, ... }`; only `name` and `func`
/// are required, every other field defaults to `PrimitiveDef::DEFAULT`.
///
/// Three forms:
///
/// - **Anonymous table** — emits `pub(crate) static PRIMITIVES: &[PrimitiveDef]`,
///   the per-module registration table:
///
///   ```ignore
///   primitive! {
///       "math/sqrt" => prim_sqrt {
///           signal: Signal::errors(), arity: Arity::Exact(1),
///           doc: "Square root.", params: &["x"],
///           category: "math", example: "(math/sqrt 16)",
///           aliases: &["sqrt"],
///       }
///       "and" => prim_and { arity: Arity::AtLeast(0), category: "logic" }
///   }
///   ```
///
/// - **Named table** — same, but a caller-chosen static name and visibility,
///   for the feature-gated side tables:
///
///   ```ignore
///   primitive!(pub(crate) static CALLBACK_PRIMITIVES =
///       "ffi/call" => prim_ffi_call { arity: Arity::AtLeast(2) }
///   );
///   ```
///
/// - **Single static** — one `static`, for the trait-method handles and the
///   ad-hoc no-op def:
///
///   ```ignore
///   primitive!(static SEQ_FIRST = "trait:Sequence:first" => trait_seq_first {
///       signal: Signal::errors(), arity: Arity::Exact(1), effect: RegionEffect::Mixed
///   });
///   ```
macro_rules! primitive {
    // Single static def. Forwards outer attributes (doc comments, `#[cfg]`).
    ( $(#[$meta:meta])* $vis:vis static $id:ident = $name:literal => $func:ident { $( $key:ident : $val:expr ),* $(,)? } ) => {
        $(#[$meta])*
        #[allow(clippy::needless_update)]
        $vis static $id: crate::primitives::def::PrimitiveDef = crate::primitives::def::PrimitiveDef {
            name: $name,
            func: $func,
            $( $key : $val, )*
            ..crate::primitives::def::PrimitiveDef::DEFAULT
        };
    };

    // Named static table. Forwards outer attributes (doc comments, `#[cfg]`).
    ( $(#[$meta:meta])* $vis:vis static $tbl:ident = $( $name:literal => $func:ident { $( $key:ident : $val:expr ),* $(,)? } )* ) => {
        $(#[$meta])*
        $vis static $tbl: &[crate::primitives::def::PrimitiveDef] = &[
            $(
                #[allow(clippy::needless_update)]
                crate::primitives::def::PrimitiveDef {
                    name: $name,
                    func: $func,
                    $( $key : $val, )*
                    ..crate::primitives::def::PrimitiveDef::DEFAULT
                }
            ),*
        ];
    };

    // Anonymous `pub(crate) static PRIMITIVES` table.
    ( $( $name:literal => $func:ident { $( $key:ident : $val:expr ),* $(,)? } )* ) => {
        pub(crate) static PRIMITIVES: &[crate::primitives::def::PrimitiveDef] = &[
            $(
                #[allow(clippy::needless_update)]
                crate::primitives::def::PrimitiveDef {
                    name: $name,
                    func: $func,
                    $( $key : $val, )*
                    ..crate::primitives::def::PrimitiveDef::DEFAULT
                }
            ),*
        ];
    };
}

/// No-op primitive: returns (SIG_OK, nil). Used by ad-hoc Value::native_fn
/// creation in tests and FFI wrappers.
fn _noop_prim(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[crate::value::Value],
) -> (crate::value::fiber::SignalBits, crate::value::Value) {
    (
        crate::value::fiber::SignalBits::EMPTY,
        crate::value::Value::NIL,
    )
}

primitive!(
    /// A silent, no-op PrimitiveDef for ad-hoc `Value::native_fn` creation.
    /// Used by tests and FFI wrappers that need a `&'static PrimitiveDef`.
    pub static NOOP_PRIM = "<noop>" => _noop_prim {
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
    }
);

/// Documentation info for a named form (primitive, special form, or macro).
/// Stored at runtime for `doc` lookup.
#[derive(Debug, Clone)]
pub struct Doc {
    pub name: &'static str,
    pub doc: &'static str,
    pub params: &'static [&'static str],
    pub arity: Arity,
    pub signal: Signal,
    pub category: &'static str,
    pub example: &'static str,
    pub aliases: &'static [&'static str],
}

impl Doc {
    /// Format as a human-readable doc string for REPL display.
    pub fn format(&self) -> String {
        let mut out = String::new();
        // Signature line
        out.push('(');
        out.push_str(self.name);
        for p in self.params {
            out.push(' ');
            out.push_str(p);
        }
        out.push(')');
        out.push('\n');
        // Description
        if !self.doc.is_empty() {
            out.push_str("  ");
            out.push_str(self.doc);
            out.push('\n');
        }
        // Arity
        out.push_str("  arity: ");
        out.push_str(&format!("{}", self.arity));
        out.push('\n');
        // Example
        if !self.example.is_empty() {
            out.push_str("  example:\n");
            for line in self.example.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        // Aliases
        if !self.aliases.is_empty() {
            out.push_str("  aliases: ");
            out.push_str(&self.aliases.join(", "));
            out.push('\n');
        }
        out
    }
}

/// Metadata extracted from primitive registration.
///
/// Returned by `register_primitives` and threaded through the
/// pipeline to the analyzer. Single source of truth for all
/// primitive metadata.
#[derive(Clone)]
pub struct PrimitiveMeta {
    pub signals: HashMap<SymbolId, Signal>,
    pub arities: HashMap<SymbolId, Arity>,
    pub docs: HashMap<SymbolId, Doc>,
    /// NativeFn values for each primitive, keyed by SymbolId.
    /// Used by `bind_primitives` to record compile-time constant
    /// values so the lowerer can emit `LoadConst` instead of
    /// `LoadGlobal`.
    pub functions: HashMap<SymbolId, Value>,
    /// Primitive SymbolId → declared [`RegionEffect`]. Aliases get the
    /// same entry as their primary name (a single map, so the alias
    /// metadata cannot drift). The region solver's call classification
    /// reads this.
    pub effects: HashMap<SymbolId, RegionEffect>,
    /// Primitive SymbolId → declared [`RetType`]. Aliases get the same entry.
    /// The ownership inference reads this to classify a `Funnel` store's
    /// container argument: a `MutableArray`/`MutableStruct` container *retains*
    /// the stored value's region (so the forest recovers a containment edge),
    /// where a `String`/`Unknown` (e.g. `@string`/`@bytes`) container copies
    /// bytes and retains nothing.
    pub ret_types: HashMap<SymbolId, RetType>,
    /// Primitive SymbolId → the argument indices it EMBEDS into its fresh result
    /// ([`PrimitiveDef::embeds`]). Aliases get the same entry. The region walk's
    /// `Fresh` arm reads this (through `CallClassification::embeds`) to record a
    /// `result ⊇ arg` containment edge for each embedded argument.
    pub embeds: HashMap<SymbolId, &'static [usize]>,
    /// Primitive SymbolId → [`PrimitiveDef::moves_out`]. Aliases get the same entry.
    /// A moves-out native's heap result is an element REMOVED from a container arg,
    /// escape-retained IN-BODY before the container release (`%pop`); the region
    /// walk reads this (through `CallClassification::moves_out`) to suppress the
    /// redundant tail ReturnValue retain — but only when the effect is also
    /// `PassThrough` (a genuinely non-fresh move-out), so a fresh grapheme/byte
    /// result keeps its retain.
    pub moves_out: HashMap<SymbolId, bool>,
}

impl PrimitiveMeta {
    pub fn new() -> Self {
        PrimitiveMeta {
            signals: HashMap::new(),
            arities: HashMap::new(),
            docs: HashMap::new(),
            functions: HashMap::new(),
            effects: HashMap::new(),
            ret_types: HashMap::new(),
            embeds: HashMap::new(),
            moves_out: HashMap::new(),
        }
    }
}

impl Default for PrimitiveMeta {
    fn default() -> Self {
        Self::new()
    }
}
