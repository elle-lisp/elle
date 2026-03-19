# plugins/base64

Base64 encoding and decoding plugin for Elle.

## Responsibility

Provide base64 encode/decode primitives:
- Standard base64 alphabet (RFC 4648 §4) with `=` padding
- URL-safe base64 alphabet (RFC 4648 §5) with no padding

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `base64/encode` | 1 | Base64-encode data (standard alphabet). Accepts string, @string, bytes, or @bytes. Returns string. |
| `base64/decode` | 1 | Base64-decode a string (standard alphabet). Accepts string or @string. Returns bytes. |
| `base64/encode-url` | 1 | Base64-encode data (URL-safe alphabet, no padding). Accepts string, @string, bytes, or @bytes. Returns string. |
| `base64/decode-url` | 1 | Base64-decode a string (URL-safe alphabet, no padding). Accepts string or @string. Returns bytes. |

## Implementation

Uses the `base64` crate (v0.22) with the `Engine` trait.
- `base64/encode` and `base64/decode` use `general_purpose::STANDARD`.
- `base64/encode-url` and `base64/decode-url` use `general_purpose::URL_SAFE_NO_PAD`.

Decode errors return `(SIG_ERROR, error_val("base64-error", msg))`.

## Building

```bash
cd plugins/base64
cargo build --release
# Output: target/release/libelle_base64.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_base64.so"))
(def encode-fn (get plugin :encode))
(encode-fn "hello")  # => "aGVsbG8="
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
