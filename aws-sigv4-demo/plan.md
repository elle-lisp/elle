# Architectural Plan: Value Representation, Binary Data, and SigV4 Demo

## Scope

Five interconnected changes, in dependency order:

1. **NaN-box tag reassignment** — optimize the hottest runtime checks by
   reassigning all 8 tags.
2. **Small string optimization (SSO)** — inline strings ≤6 UTF-8 bytes in the
   Value, eliminating heap allocation for single-character access.
3. **Bytes and blob types** — new value types for binary data.
4. **Crypto primitives** — SHA-256 and HMAC-SHA256 backed by pure Rust crates.
5. **SigV4 demo** — idiomatic Elle translation of the Scheme reference
   implementation, exercising all of the above.

### What this plan does NOT do

**Strings and buffers are unchanged semantically.** Elle's strings and buffers
are sequences of human-perceived grapheme clusters. `get` on a string returns a
length-1 string. `get` on a buffer returns a length-1 buffer. `push`/`pop` on
buffers return length-1 buffers. These semantics are not negotiable and this
plan does not touch them.

Elle is not Janet. Janet treats strings as byte sequences. Elle treats strings
as text. The binary data abstraction in Elle is the new bytes type, not strings
or buffers.

---

## Part 1: NaN-box Tag Reassignment

### Problem

The current tag assignment wastes dedicated tags on infrequent operations
(symbol, keyword) while forcing the most frequent checks (truthiness, nil,
empty-list) through payload comparisons. All five singletons (nil, false, true,
empty-list, undefined) share tag `0x7FFC` and are distinguished only by
payload. Truthiness — the single hottest check in the VM — requires both a tag
comparison and a payload comparison.

### New tag layout

| Tag | Name | Contents |
|-----|------|----------|
| `0x7FF8` | Integer | 48-bit signed integer. Unchanged. |
| `0x7FF9` | Falsy | payload 0 = nil, payload 1 = false. |
| `0x7FFA` | Empty-list | No payload. Tag-only check. |
| `0x7FFB` | Heap pointer | All heap-allocated types. Unchanged. |
| `0x7FFC` | Truthy singletons | payload 0 = true, payload 1 = undefined. |
| `0x7FFD` | NaN/Infinity | IEEE 754 special float values. Unchanged. |
| `0x7FFE` | Small values | Sub-tagged: symbol, keyword, C pointer. |
| `0x7FFF` | SSO string | 6 bytes inline UTF-8. See Part 2. |

### Concrete bit patterns

| Value | Bits |
|-------|------|
| nil | `0x7FF9_0000_0000_0000` |
| false | `0x7FF9_0000_0000_0001` |
| true | `0x7FFC_0000_0000_0000` |
| undefined | `0x7FFC_0000_0000_0001` |
| empty-list | `0x7FFA_0000_0000_0000` |

### Performance wins

**Truthiness** becomes `(bits >> 48) != 0x7FF9` — one shift, one compare, no
payload inspection. This is the single hottest check in the VM. Today it
requires tag comparison plus payload comparison against both nil and false.

**`is_nil()`** remains a full u64 compare (`bits == 0x7FF9_0000_0000_0000`),
same cost as today.

**`is_empty_list()`** becomes `(bits >> 48) == 0x7FFA` — tag-only, down from
tag plus payload comparison.

### Sub-tagging within `0x7FFE`

Symbol, keyword, and C pointer are infrequent at runtime. Symbol almost never
appears (bindings are resolved at analysis time). Keyword is moderate (struct
and table keys). C pointer is rare (FFI only). The sub-tag cost is noise at
these frequencies.

Use a few high bits of the 48-bit payload as discriminant. For example, bits
46-47 distinguish symbol (00), keyword (01), C pointer (10), leaving 46 bits
for payload. Symbols need 32 bits. Keywords need a pointer — on x86-64 only 47
bits of virtual address are canonical, and interned pointers are in low memory
so fewer bits suffice. C pointers need 47 canonical bits. Work out the exact
sub-tag scheme during implementation.

### Migration

This is a representation change, not a semantic change. All existing tests must
produce identical results. The public API (`Value::nil()`, `Value::bool()`,
`is_nil()`, `is_truthy()`, etc.) stays the same; only the bit patterns behind
them change.

---

## Part 2: Small String Optimization (SSO)

### Problem

Every `char-at` and `get` on a string currently allocates a heap string: the
character is converted to a Rust `String`, then interned via HashMap lookup into
an `Rc<HeapObject>`. For single ASCII characters this is a 1-byte string going
through full interning machinery. This is the dominant cost of character-level
string operations.

### Design

Strings ≤6 bytes of UTF-8 are stored inline in the Value using tag `0x7FFF`,
with no heap allocation, no interning, no `Rc`, no pointer chase.

**Coverage:** All ASCII characters (1 byte), Latin-1 and common diacritics (2-3
bytes), most CJK (3 bytes), most emoji (4 bytes), and many multi-codepoint
grapheme clusters (5-6 bytes). The vast majority of single-grapheme-cluster
strings that `char-at` and `get` return.

**What still goes to heap:** Strings longer than 6 UTF-8 bytes. Flag emoji (8
bytes), family emoji (25 bytes), and other long grapheme clusters. All
multi-cluster strings.

### This is a representation optimization, not a type change

`type-of` returns `"string"` for both inline and heap strings. All string
operations work on both. The programmer never sees "SSO string" as a concept.
`Value::string()` decides which representation to use based on byte length — ≤6
goes inline, >6 goes to heap interning as today.

### Encoding within the 48-bit payload

The 6-byte inline capacity is a hard requirement, not a nice-to-have. If the
encoding scheme cannot cleanly fit 6 bytes of UTF-8 into the 48-bit payload, we
return to planning and re-evaluate rather than silently accepting 5 bytes.

6 bytes = 48 bits exactly. But length must also be recoverable. 3 bits for
length (0-6) leaves 45 bits = 5 bytes + 5 bits — not enough for 6 full bytes.
Options: length-from-content scheme (store bytes left-aligned, determine length
by trailing zeros), trailing sentinel, or exploiting the fact that UTF-8 strings
cannot contain zero bytes (the only UTF-8 byte that is 0x00 is the NUL
character). A zero-terminated scheme works for all valid UTF-8. The exact
encoding is an implementation detail to be resolved during implementation.

### Equality

Two SSO strings compare as `self.0 == other.0` — a single u64 comparison. This
is the fast path in the existing equality code (the `!is_heap() && !is_heap()`
branch already does bit comparison).

SSO string vs heap string: one is non-heap, one is heap — the current code
returns false for this case. This is correct only if `Value::string()` always
produces SSO for short strings. A string that fits in SSO must never exist as a
heap string.

### Consistency requirement

`Value::string()` must always produce SSO for strings that fit. The interning
path is only used for longer strings. This means short strings are no longer
interned — but they don't need to be, because u64 equality is cheaper than
HashMap lookup.

### Impact on `char-at` and `get`

These currently allocate a heap string for every character access. With SSO,
they return an inline Value — no allocation, no interning, no HashMap lookup.
This is the primary motivation for SSO.

---

## Part 3: Bytes and Blob Types

### Elle's type model (not negotiable)

- Strings and buffers are sequences of human-perceived grapheme clusters. They
  are text. Their element operations (`get`, `put`, `push`, `pop`) work on
  grapheme clusters.
- Bytes and blob are sequences of 8-bit integers. They are binary data. Their
  element operations work on integers 0-255.
- Conversion: string/buffer → bytes always succeeds (UTF-8 encoding). Bytes →
  string is fallible (must be valid UTF-8).

### Two types, following Elle's mutable/immutable convention

**Bytes** (immutable): `Rc<Vec<u8>>`, heap-allocated via `TAG_POINTER` as a new
`HeapObject` variant. The frozen binary value — crypto digests, serialized
messages, protocol data. Analogous to string (immutable text).

**Blob** (mutable): `Rc<RefCell<Vec<u8>>>`, heap-allocated via `TAG_POINTER` as
a new `HeapObject` variant. The binary workspace — building packets,
accumulating encoded data, FFI buffers. Analogous to buffer (mutable text).

No new NaN-box tags consumed. Both are heap types under `TAG_POINTER`.

### Primitives

**Construction (no conversion):**
- `bytes` — variadic constructor, each argument integer 0-255. Returns
  immutable bytes.
- `blob` — variadic constructor. Returns mutable blob.

**Text → binary (always succeeds, UTF-8 encoding):**
- `string->bytes` — string to immutable bytes.
- `string->blob` — string to mutable blob.
- `buffer->bytes` — buffer to immutable bytes.
- `buffer->blob` — buffer to mutable blob.

**Binary → text (fallible, UTF-8 validation):**
- `bytes->string` — bytes to string. Error on invalid UTF-8.
- `bytes->buffer` — bytes to buffer. Error on invalid UTF-8.
- `blob->string` — blob to string. Error on invalid UTF-8.
- `blob->buffer` — blob to buffer. Error on invalid UTF-8.

**Binary ↔ binary (copy ± mutability):**
- `blob->bytes` — freeze a blob to immutable bytes (copies).
- `bytes->blob` — thaw bytes to mutable blob (copies).

**Presentation:**
- `bytes->hex` / `bytes->hex-string` — bytes to lowercase hex string.
- `blob->hex` / `blob->hex-string` — blob to lowercase hex string.

**Access (polymorphic, dispatch on type):**
- `get` on bytes/blob — returns integer 0-255. Out of bounds returns nil.
- `put` on blob — sets byte at index. Blob only (mutable).
- `push` on blob — append byte. Blob only.
- `pop` on blob — remove and return last byte as integer. Blob only.
- `length` on bytes/blob — number of bytes.

**Collection operations (same as other collection types):**
- `slice` on bytes/blob — returns new value of same type.
- `append` on bytes/blob — concatenate two values. Returns same type as first
  argument.
- `each` on bytes/blob — iterate, binding each byte as integer.
- `map` over bytes/blob — returns list of integers (consistent with map
  returning a list for all collection types).

**Equality:** `=` on bytes/blob compares contents.

**Type predicates:**
- `bytes?` — returns true for bytes values, false otherwise.
- `blob?` — returns true for blob values, false otherwise.

**Display:** Bytes displays as a hex-based representation (exact syntax is an
implementation detail). Blob similarly, with a mutable marker.

### Polymorphic `keys`, `values`, `del`

`keys`, `values`, and `del` must be polymorphic — they dispatch on type and work
on both structs and tables. If there is currently a `struct/del` primitive, it
should be removed in favor of the polymorphic `del`. This is a cleanup of
existing code, not new functionality for bytes/blob (these operations don't
apply to byte sequences).

### `uri-encode`

Takes a string, returns a percent-encoded string per RFC 3986. Unreserved
characters (A-Z, a-z, 0-9, `-`, `.`, `_`, `~`) pass through unchanged. All
other characters are percent-encoded as `%XX` with uppercase hex, which is
RFC 3986 compliant.

This is a Rust primitive for correctness and speed. URI encoding is hot in any
HTTP-facing code, the RFC 3986 spec has subtleties (which characters are
unreserved, hex case, multi-byte UTF-8 encoding producing multiple `%XX`
sequences), and getting it right matters more than getting it fast — but a Rust
implementation gets both. Aliased as `uri-encode`.

Pure effect — deterministic function of its input.

### Design rationale

**Why new types instead of reusing buffers?** Buffers are mutable text sequences
(grapheme clusters). Bytes are immutable binary sequences (integers). These are
different abstractions with different element types, different indexing
semantics, and different use cases. Conflating them would require buffers to
have two personalities — text when used with `push`/`pop`/`get`, binary when
used with crypto. That's the kind of semantic ambiguity this plan exists to
prevent.

**Why not tuples of integers?** A tuple of integers could represent binary data,
but it lacks identity — there's no way to dispatch on "this is binary data" vs
"this is a tuple of numbers." Crypto primitives need to know they're receiving
binary data, not an arbitrary tuple. A dedicated type makes the intent explicit
and enables proper type checking.

**Why both bytes and blob?** Elle's mutable/immutable split is a core design
principle: string/buffer, tuple/array, struct/table. Binary data should follow
the same pattern. Crypto digests are values (immutable). FFI buffers and packet
builders are workspaces (mutable). Providing only one forces users to choose
between safety and convenience.

**Why no interning for bytes?** Unlike strings (where the same identifiers and
keywords appear thousands of times), byte sequences are typically unique —
crypto digests, network packets, serialized data. Hashing on every freeze for
near-zero dedup benefit is wasted work. If profiling later reveals significant
byte sequence duplication, interning can be added without changing the public
API.

---

## Part 4: Crypto Primitives

`sha2` and `hmac` from the RustCrypto project. Pure Rust, no C dependencies,
no unsafe.

### `crypto/sha256`

Takes a string or bytes. When given a string, hashes its UTF-8 encoding. When
given bytes, hashes the raw bytes. Returns bytes (32 bytes, the raw SHA-256
digest). Not a hex string — raw bytes. Hex encoding is the caller's
responsibility via `bytes->hex`.

Accepting both string and bytes avoids forcing callers to write
`(string->bytes s)` for the common case of hashing text. The primitive extracts
the underlying byte slice from either type.

Aliased as `sha256` for convenience. The namespaced form `crypto/sha256` is
canonical, following Elle's convention for namespaced primitives (`ffi/native`,
`bit/and`, `string/trim`).

Pure effect — deterministic function of its input.

### `crypto/hmac-sha256`

Takes (key, message) — each is string or bytes. Returns bytes (32 bytes, the
raw HMAC-SHA256 MAC). Key is the first argument, message is the second. This
matches the conventional HMAC(key, message) argument order used by AWS SigV4
and most crypto APIs.

Same input flexibility as `sha256`. The signing key derivation chain passes
bytes output as the key to the next HMAC call — this works naturally because
the primitives accept bytes.

Aliased as `hmac-sha256`. Pure effect.

### Design rationale

**Why return bytes, not hex strings?** The HMAC output is often fed as the key
into another HMAC call (SigV4's signing key derivation is exactly this:
HMAC(HMAC(HMAC(...)))). Returning raw bytes avoids hex-encode/hex-decode
round-trips between chained calls. The caller hex-encodes only at the final
presentation step.

**Why return bytes, not blob?** Crypto digests are values, not workspaces. You
pass them around, compare them, hex-encode them. You never mutate a SHA-256
digest. Immutable by default.

**Why accept both string and bytes?** The demo hashes strings (canonical
request, string-to-sign) and chains bytes (signing key derivation). Supporting
both at the primitive level avoids ceremony. The alternative — requiring
explicit `string->bytes` — adds noise without adding safety.

### Testing

Rust-side unit tests against known test vectors (RFC 4231 for HMAC-SHA256,
FIPS 180-4 for SHA-256). These are deterministic — expected outputs are
constants. The Elle demo serves as an integration test.

---

## Part 5: SigV4 Demo

Direct translation of the Scheme reference implementation into idiomatic Elle.
Uses real crypto primitives instead of Scheme's zero-filled placeholders.

### Key design decisions (Elle idioms replacing Scheme idioms)

1. **Named `let` → `letrec` with tail-recursive helper.** The Scheme source
   uses `(let loop ((i 0)) ...)` in `char-in-string?` and `to-hex-string`.
   Elle has no named `let`. Replace with `letrec` binding a named function,
   then call it. This is the established Elle idiom (used in the prelude for
   `gen-params`). Do NOT use `while` with mutable variables — the Scheme
   originals accumulate through recursion.

2. **Characters → single-character strings.** Elle has no character type.
   `char-at` returns a single-character string. Character literals become
   string literals: `#\a` → `"a"`, `#\0` → `"0"`. `(char=? x y)` → `(= x y)`
   (equality works on strings).

3. **URI encoding → `uri-encode` primitive.** The Scheme demo's
   `percent-encode-char`, `uri-unreserved?`, `char-in-string?`, and
   `to-hex-string` are all replaced by a single `(uri-encode s)` call. This
   eliminates the need for `chr`, character range checks, and hex formatting
   helpers. The primitive produces uppercase hex (`%2F`), which is RFC 3986
   correct — the Scheme demo's lowercase (`%2f`) is not.

4. **`(apply string-append ...)` → `(string-join ... "")`.** Mechanical. Every
   `(apply string-append (map f lst))` becomes `(string-join (map f lst) "")`.

5. **Variadic `string-append` → `string-join` with list.** Multi-argument
   string concatenation uses `(string-join (list ...) "")`.

6. **`make-string` → recursive padding.** `pad-int` uses `letrec` with a
   recursive helper that prepends `"0"` until the result reaches the desired
   width.

7. **`for-each` → `each`.** Scheme's `(for-each (lambda (x) ...) lst)` →
   Elle's `(each x lst ...)`.

8. **`null?` → `empty?`.** Per AGENTS.md: lists are `EMPTY_LIST`-terminated,
   use `empty?`.

9. **Booleans: `#t`/`#f` → `true`/`false`.** Mechanical.

10. **`car`/`cdr` → `first`/`rest`.** Mechanical.

11. **`string->number` → `string->int`.** All parsed values are integers.

12. **`bytevector->hex-string` → `bytes->hex`.** The crypto primitives return
    bytes, `bytes->hex` converts to a hex string. Clean data flow, no
    intermediate representations.

13. **`signed-headers-list` rewritten** with `string-join` and `map`, same
    logic as the Scheme version but using Elle idioms.

14. **`canonical-query-string` rewritten** with `string-join`.

### What the demo validates

String manipulation (`substring`, `string-downcase`, `string-trim`,
`string-join`, `append`, `char-at`, `length`, `number->string`, `string->int`).
URI encoding (`uri-encode` for RFC 3986 percent-encoding). Bytes type
(`bytes->hex` for hex encoding of crypto output). Crypto primitives
(`crypto/sha256` and `crypto/hmac-sha256` on string and bytes inputs, chained
HMAC for signing key derivation). Higher-order functions (`map` over lists,
closures). Recursive patterns (`letrec` with tail-recursive helpers). List
operations (`cons`, `first`, `rest`, `reverse`, `empty?`, `list`). Control flow
(`cond` with `else`, `if`, `and`, `or`). Formatted output (`display`,
`newline`).

### Expected output

Matches Scheme version for non-crypto tests except URI encoding. Produces real
SHA-256 and HMAC-SHA256 output for crypto tests. URI percent-encoding uses
uppercase hex (`%2F` not `%2f`) — this is RFC 3986 correct. The Scheme demo
uses lowercase, which is technically valid but not the canonical form. We are
correct; the Scheme version isn't.

### Scheme reference cleanup

Update `demos/aws-sigv4/sigv4.scm` to use uppercase hex in percent-encoding
output (`%2F` not `%2f`) to match RFC 3986. The Scheme reference implementation
should be correct.

---

## Risks

1. **Tag reassignment is invasive.** Touches every Value constructor, every type
   check, every match on Value. Mitigated by: identical public API, all existing
   tests must pass unchanged.

2. **SSO consistency.** `Value::string()` must always choose inline for strings
   that fit. If both representations can exist for the same content, equality
   breaks. Mitigated by: single constructor path, no way to force heap
   allocation for short strings.

3. **SSO length encoding.** Getting 6 full bytes into 48 bits while also
   encoding length needs careful bit-level design. 6 bytes is a hard
   requirement — if the encoding proves impractical, we re-plan rather than
   silently downgrade to 5.

4. **Bytes and blob add two new `HeapObject` variants.** Touches display,
   equality, type-of, get/put/push/pop dispatch. Moderate implementation
   surface but follows established patterns.

5. **Crate version compatibility.** `sha2` and `hmac` share the `digest` trait
   crate. Pin if version resolution fails.

---

## Ordering

1. **Tag reassignment (Part 1)** first — foundational, everything else builds
   on it.
2. **SSO (Part 2)** — depends on the new tag layout having `0x7FFF` reserved
   for inline strings.
3. **Bytes and blob (Part 3)** — independent of SSO but benefits from the
   stable tag layout.
4. **Crypto (Part 4)** — depends on the bytes type.
5. **SigV4 demo (Part 5)** — depends on everything above.

Parts 2 and 3 could be parallelized. Part 1 must come first.
