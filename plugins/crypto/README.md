# elle-crypto

A crypto plugin for Elle, providing SHA-2 family hashes and HMAC variants via the Rust `sha2` and `hmac` crates.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_crypto.so` (or `target/release/libelle_crypto.so`).

## Usage

```lisp
(import-file "path/to/libelle_crypto.so")

(bytes->hex (crypto/sha256 "hello"))
;; => "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"

(bytes->hex (crypto/hmac-sha256 "key" "message"))
;; => "6e9ef29b75fffc5b7abae527d58fdadb2fe42e7219011976917343065f58ed4a"
```

## Primitives

### Hash functions

All accept string, bytes, or blob input.

| Name | Output size | Description |
|------|-------------|-------------|
| `crypto/sha224` | 28 bytes | SHA-224 hash |
| `crypto/sha256` | 32 bytes | SHA-256 hash |
| `crypto/sha384` | 48 bytes | SHA-384 hash |
| `crypto/sha512` | 64 bytes | SHA-512 hash |
| `crypto/sha512-224` | 28 bytes | SHA-512/224 (SHA-512 truncated to 224 bits) |
| `crypto/sha512-256` | 32 bytes | SHA-512/256 (SHA-512 truncated to 256 bits) |

### HMAC functions

All take (key, message) where each is string, bytes, or blob.

| Name | Output size | Description |
|------|-------------|-------------|
| `crypto/hmac-sha224` | 28 bytes | HMAC-SHA224 |
| `crypto/hmac-sha256` | 32 bytes | HMAC-SHA256 |
| `crypto/hmac-sha384` | 48 bytes | HMAC-SHA384 |
| `crypto/hmac-sha512` | 64 bytes | HMAC-SHA512 |
| `crypto/hmac-sha512-224` | 28 bytes | HMAC-SHA512/224 |
| `crypto/hmac-sha512-256` | 32 bytes | HMAC-SHA512/256 |
