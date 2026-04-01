# Elle wasmparser patch

## What

`MAX_WASM_FUNCTION_SIZE` raised from 128KB to 128MB in `src/limits.rs`.
`MAX_WASM_BR_TABLE_SIZE` follows (set to `MAX_WASM_FUNCTION_SIZE`).

## Why

Elle compiles stdlib + user code into a single WASM module where the entry
function contains the entire program as one large function body. With ~200
stdlib closures plus user code, the entry function regularly exceeds the
upstream 128KB limit.

## Plan to eliminate

Split stdlib into separately compiled WASM modules that are imported at
link time. Each module stays well under the limit. This removes the need
for the patch entirely. Tracked as post-merge work.
