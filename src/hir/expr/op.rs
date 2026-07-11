/// Known %-intrinsic operations with fixed type/alloc/escape behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Mod,
    // Comparison
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
    // Logical
    Not,
    // Conversion
    Int,
    Float,
    // Pair operations
    Pair, // pair constructor: allocates
    First,
    Rest,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,
    // Comparison (missing)
    Ne,
    // Type predicates
    IsNil,
    IsEmpty,
    IsBool,
    IsInt,
    IsFloat,
    IsString,
    IsKeyword,
    IsSymbol,
    IsPair,
    IsArray,
    IsStruct,
    IsSet,
    IsBytes,
    IsBox,
    IsClosure,
    IsFiber,
    TypeOf,
    // Data access
    Length,
    Get,
    Put,
    /// Monomorphic struct/array put, immutable input: fresh immutable twin
    /// (`%put-struct` / `%put-array`). Precise `Struct`/`Array` return + `Fresh`
    /// effect, distinct from the polymorphic `%put`.
    PutStruct,
    PutArray,
    /// Monomorphic struct/array put, mutable input: in-place funnel store returning
    /// arg0 (`%put-struct-mut` / `%put-array-mut`, `MutableStruct`/`MutableArray`).
    /// `-mut` is valid only on a proven mutable container (the monomorphization
    /// contract).
    PutStructMut,
    PutArrayMut,
    /// Monomorphic set add: `%add-set` inserts into an immutable set, returning a
    /// fresh `Set`; `%add-set-mut` inserts into a mutable `@set` in place, returning
    /// it. Both share the polymorphic `add`/`prim_add` runtime body (which freezes
    /// the element and dispatches on the container's runtime mutability); the precise
    /// `Set`/`MutableSet` return is the monomorphization win. These are the silent
    /// funnel natives the stdlib `add` type-dispatch wrapper lowers to on a proven
    /// set — the set-family peer of `%put-*`/`%push-*`.
    AddSet,
    AddSetMut,
    Del,
    Has,
    Push,
    /// Monomorphic array push, immutable input: `%push-array` — returns a fresh
    /// immutable `Array` twin (the polymorphic `%array-push`'s immutable case made
    /// a distinct op so it can carry a precise `Array` return type / `Fresh` effect).
    PushArray,
    /// Monomorphic array push, mutable input: `%push-array-mut` — in-place funnel
    /// store, returns arg0 (`MutableArray`). The mutability the name pins is the
    /// monomorphization contract: the `-mut` variant is
    /// only valid on a proven `@array`.
    PushArrayMut,
    Pop,
    /// Append string to @string (or create new string).
    StringPush,
    /// Append byte to @bytes (or create new bytes).
    BytesPush,
    // Mutability
    Freeze,
    Thaw,
    // Identity
    Identical,
}

impl IntrinsicOp {
    /// Name as it appears in source code (with % prefix).
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "%add",
            Self::Sub => "%sub",
            Self::Mul => "%mul",
            Self::Div => "%div",
            Self::Rem => "%rem",
            Self::Mod => "%mod",
            Self::Eq => "%eq",
            Self::Lt => "%lt",
            Self::Gt => "%gt",
            Self::Le => "%le",
            Self::Ge => "%ge",
            Self::Not => "%not",
            Self::Int => "%int",
            Self::Float => "%float",
            Self::Pair => "%pair",
            Self::First => "%first",
            Self::Rest => "%rest",
            Self::BitAnd => "%bit-and",
            Self::BitOr => "%bit-or",
            Self::BitXor => "%bit-xor",
            Self::BitNot => "%bit-not",
            Self::Shl => "%shl",
            Self::Shr => "%shr",
            Self::Ne => "%ne",
            Self::IsNil => "%nil?",
            Self::IsEmpty => "%empty?",
            Self::IsBool => "%bool?",
            Self::IsInt => "%int?",
            Self::IsFloat => "%float?",
            Self::IsString => "%string?",
            Self::IsKeyword => "%keyword?",
            Self::IsSymbol => "%symbol?",
            Self::IsPair => "%pair?",
            Self::IsArray => "%array?",
            Self::IsStruct => "%struct?",
            Self::IsSet => "%set?",
            Self::IsBytes => "%bytes?",
            Self::IsBox => "%box?",
            Self::IsClosure => "%closure?",
            Self::IsFiber => "%fiber?",
            Self::TypeOf => "%type-of",
            Self::Length => "%length",
            Self::Get => "%get",
            Self::Put => "%put",
            Self::PutStruct => "%put-struct",
            Self::PutArray => "%put-array",
            Self::PutStructMut => "%put-struct-mut",
            Self::PutArrayMut => "%put-array-mut",
            Self::AddSet => "%add-set",
            Self::AddSetMut => "%add-set-mut",
            Self::Del => "%del",
            Self::Has => "%has?",
            Self::Push => "%array-push",
            Self::PushArray => "%push-array",
            Self::PushArrayMut => "%push-array-mut",
            Self::Pop => "%pop",
            Self::StringPush => "%string-push",
            Self::BytesPush => "%bytes-push",
            Self::Freeze => "%freeze",
            Self::Thaw => "%thaw",
            Self::Identical => "%identical?",
        }
    }

    /// Look up an intrinsic by its %-prefixed name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "%add" => Some(Self::Add),
            "%sub" => Some(Self::Sub),
            "%mul" => Some(Self::Mul),
            "%div" => Some(Self::Div),
            "%rem" => Some(Self::Rem),
            "%mod" => Some(Self::Mod),
            "%eq" => Some(Self::Eq),
            "%lt" => Some(Self::Lt),
            "%gt" => Some(Self::Gt),
            "%le" => Some(Self::Le),
            "%ge" => Some(Self::Ge),
            "%not" => Some(Self::Not),
            "%int" => Some(Self::Int),
            "%float" => Some(Self::Float),
            "%pair" => Some(Self::Pair),
            "%first" => Some(Self::First),
            "%rest" => Some(Self::Rest),
            "%bit-and" => Some(Self::BitAnd),
            "%bit-or" => Some(Self::BitOr),
            "%bit-xor" => Some(Self::BitXor),
            "%bit-not" => Some(Self::BitNot),
            "%shl" => Some(Self::Shl),
            "%shr" => Some(Self::Shr),
            "%ne" => Some(Self::Ne),
            "%nil?" => Some(Self::IsNil),
            "%empty?" => Some(Self::IsEmpty),
            "%bool?" => Some(Self::IsBool),
            "%int?" => Some(Self::IsInt),
            "%float?" => Some(Self::IsFloat),
            "%string?" => Some(Self::IsString),
            "%keyword?" => Some(Self::IsKeyword),
            "%symbol?" => Some(Self::IsSymbol),
            "%pair?" => Some(Self::IsPair),
            "%array?" => Some(Self::IsArray),
            "%struct?" => Some(Self::IsStruct),
            "%set?" => Some(Self::IsSet),
            "%bytes?" => Some(Self::IsBytes),
            "%box?" => Some(Self::IsBox),
            "%closure?" => Some(Self::IsClosure),
            "%fiber?" => Some(Self::IsFiber),
            "%type-of" => Some(Self::TypeOf),
            "%length" => Some(Self::Length),
            "%get" => Some(Self::Get),
            "%put" => Some(Self::Put),
            "%put-struct" => Some(Self::PutStruct),
            "%put-array" => Some(Self::PutArray),
            "%put-struct-mut" => Some(Self::PutStructMut),
            "%put-array-mut" => Some(Self::PutArrayMut),
            "%add-set" => Some(Self::AddSet),
            "%add-set-mut" => Some(Self::AddSetMut),
            "%del" => Some(Self::Del),
            "%has?" => Some(Self::Has),
            "%array-push" => Some(Self::Push),
            "%push-array" => Some(Self::PushArray),
            "%push-array-mut" => Some(Self::PushArrayMut),
            "%pop" => Some(Self::Pop),
            "%string-push" => Some(Self::StringPush),
            "%bytes-push" => Some(Self::BytesPush),
            "%freeze" => Some(Self::Freeze),
            "%thaw" => Some(Self::Thaw),
            "%identical?" => Some(Self::Identical),
            _ => None,
        }
    }

    /// Required arity (min, max). Most are fixed; %sub allows 1 or 2.
    pub fn arity(self) -> (usize, usize) {
        match self {
            Self::Not
            | Self::Int
            | Self::Float
            | Self::First
            | Self::Rest
            | Self::BitNot
            | Self::IsNil
            | Self::IsEmpty
            | Self::IsBool
            | Self::IsInt
            | Self::IsFloat
            | Self::IsString
            | Self::IsKeyword
            | Self::IsSymbol
            | Self::IsPair
            | Self::IsArray
            | Self::IsStruct
            | Self::IsSet
            | Self::IsBytes
            | Self::IsBox
            | Self::IsClosure
            | Self::IsFiber
            | Self::TypeOf
            | Self::Length
            | Self::Pop
            | Self::Freeze
            | Self::Thaw => (1, 1),
            Self::Sub => (1, 2),
            Self::Add
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::Mod
            | Self::Eq
            | Self::Ne
            | Self::Lt
            | Self::Gt
            | Self::Le
            | Self::Ge
            | Self::Pair
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::Shl
            | Self::Shr
            | Self::Get
            | Self::Del
            | Self::Has
            | Self::Push
            | Self::PushArray
            | Self::PushArrayMut
            | Self::AddSet
            | Self::AddSetMut
            | Self::StringPush
            | Self::BytesPush
            | Self::Identical => (2, 2),
            Self::Put
            | Self::PutStruct
            | Self::PutArray
            | Self::PutStructMut
            | Self::PutArrayMut => (3, 3),
        }
    }

    /// Does this intrinsic allocate heap memory at this HIR node?
    ///
    /// True iff the lowerer for this op uses `emit_alloc` (which
    /// annotates the resulting instruction with a region id) — i.e.
    /// the runtime creates a new heap value in a region owned by
    /// this HIR node. `Put`/`Del` mutate the input collection
    /// in place; `Get`/`Length`/`TypeOf` return a value or
    /// immediate that already lives somewhere (or doesn't need a
    /// region at all); see the Intrinsic walk in `regions.rs` for
    /// how those flow.
    pub fn allocates(self) -> bool {
        matches!(self, Self::Pair | Self::Freeze | Self::Thaw)
    }

    /// Does this op lower as the **native funnel `Call`** rather than an
    /// inline opcode (docs/intrinsics.md § Lowering)? The storing, removing,
    /// and copying ops ride the escape-correct native path: their region
    /// accounting (cross-region edge recording, call-result regions, `%pop`'s
    /// moved-out element) lives in the natives. The analyzer routes a
    /// call-position use of these through `analyze_call` to the registered
    /// NativeFn; everything else becomes an `Intrinsic` opcode node.
    pub fn routes_native_funnel(self) -> bool {
        matches!(
            self,
            Self::Put
                | Self::PutStruct
                | Self::PutArray
                | Self::PutStructMut
                | Self::PutArrayMut
                | Self::AddSet
                | Self::AddSetMut
                | Self::Push
                | Self::PushArray
                | Self::PushArrayMut
                | Self::StringPush
                | Self::BytesPush
                | Self::Del
                | Self::Pop
                | Self::Freeze
                | Self::Thaw
        )
    }

    /// Does this intrinsic produce a **call-result region** — a fresh per-call
    /// region for a conditionally-allocating native (`%put`/`%del`/
    /// `%string-push`/`%array-push`/`%bytes-push`) whose result is freed *by
    /// value* (`DecrefValueRegion`), exactly like a `Call`? Distinct from
    /// [`allocates`](Self::allocates), which drives the static-slot
    /// `emit_alloc`/`DecrefRegion` model used by the *unconditionally*-
    /// allocating `%pair`/`%freeze`/`%thaw`. These ops
    /// mint their region inside the opcode handler and pass-through-retain
    /// (`src/vm/types.rs::run_alloc_intrinsic`); the region walk marks the
    /// result a `call_result_region`, and `Hir::allocates` reports `true` so
    /// ANF names the result — without that synthetic binding the result has no
    /// slot and its `DecrefValueRegion` is orphaned when the value is consumed
    /// as an operand / discarded (the orphan-leak this predicate prevents).
    pub fn produces_call_result_region(self) -> bool {
        matches!(
            self,
            Self::Put
                | Self::PutStruct
                | Self::PutArray
                | Self::PutStructMut
                | Self::PutArrayMut
                | Self::AddSet
                | Self::AddSetMut
                | Self::Del
                | Self::StringPush
                | Self::Push
                | Self::PushArray
                | Self::PushArrayMut
                | Self::BytesPush
        )
    }
}
