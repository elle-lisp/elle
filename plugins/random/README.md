# elle-random

A fast pseudo-random number generator plugin for Elle, wrapping the Rust `fastrand` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_random.so` (or `target/release/libelle_random.so`).

## Usage

```lisp
(import-file "path/to/libelle_random.so")

(random/seed 42)              ;; deterministic from here
(random/int)                  ;; random integer (full range)
(random/int 100)              ;; 0..100
(random/int 10 20)            ;; 10..20
(random/float)                ;; 0.0..1.0
(random/bool)                 ;; true or false
(random/bytes 16)             ;; 16 random bytes
(random/shuffle @[1 2 3 4 5]) ;; shuffled array
(random/choice @[1 2 3 4 5])  ;; random element
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `random/seed` | seed (integer) | nil |
| `random/int` | [max] or [min, max] | integer |
| `random/float` | — | float (0..1) |
| `random/bool` | — | boolean |
| `random/bytes` | length | bytes |
| `random/shuffle` | array or tuple | new shuffled array |
| `random/choice` | array or tuple | random element |
