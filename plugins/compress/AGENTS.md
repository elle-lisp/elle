# plugins/compress

Compression and decompression via gzip, deflate (raw DEFLATE), and zstd.

## Responsibility

Provides six primitives for lossless data compression and decompression.
All primitives accept string, @string, bytes, or @bytes as input and return
bytes. Compression primitives accept an optional integer compression level.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `compress/gzip` | Range(1,2) | Gzip-compress data. Optional level 0–9 (default 6). |
| `compress/gunzip` | Exact(1) | Gzip-decompress data. |
| `compress/deflate` | Range(1,2) | Raw DEFLATE compress. Optional level 0–9 (default 6). |
| `compress/inflate` | Exact(1) | Raw DEFLATE decompress. |
| `compress/zstd` | Range(1,2) | Zstd-compress data. Optional level 1–22 (default 3). |
| `compress/unzstd` | Exact(1) | Zstd-decompress data. |

## Implementation

Uses the `flate2` crate for gzip and deflate, and the `zstd` crate for zstd.

Input is extracted via `extract_byte_data`, which accepts string, @string,
bytes, or @bytes (strings are treated as their UTF-8 byte representation).

Compression level is optional. When omitted, defaults are used (6 for
gzip/deflate, 3 for zstd). When provided, validated against the allowed range;
out-of-range integers return `compress-error`, non-integers return `type-error`.

All decompression errors return `compress-error`. All input type mismatches
return `type-error`.

## Building

```bash
cd plugins/compress
cargo build --release
# Output: target/release/libelle_compress.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_compress.so"))
(def gzip-fn (get plugin :gzip))
(gzip-fn "hello")
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies (flate2, zstd) |
| `src/lib.rs` | Plugin implementation |
