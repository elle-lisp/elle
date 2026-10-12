# Cranelift

<!-- audited: 2026-10-06 -->

The JIT and the WASM backend share one Cranelift code generator, which every build compiles optimized.

## Debug builds optimize dependencies

A **debug** build applies

```toml
[profile.dev.package."*"]
opt-level = 3
```

so every dependency — `cranelift-codegen`/`cranelift-frontend`/`regalloc2`
included — compiles optimized while `elle` itself stays at `opt-level = 0` with
full debug assertions. Without the override those crates build at `opt-level =
0`, where Cranelift's hot SSA-construction and bitset ops are un-inlined
call-per-op (profiled: `cranelift_bitset` / `SSABuilder` self-time dominates)
and one stdlib module costs seconds of wasmtime compile where the override
costs a fraction of one. (`--cache=<dir>` amortizes either way.)

`cranelift-codegen` is shared between the WASM path (via `wasmtime`) and the
**Cranelift JIT** (`cranelift-jit`), so optimizing it also speeds up JIT
compilation. The generated JIT *code* is unchanged: the JIT pins its own output
at `opt_level = "speed"` ([compiler.rs](../../src/jit/compiler.rs)) independent
of how `cranelift-codegen` was itself built, so the tier is behavior-identical
across the override — only compile *latency* moves. A faster background compile
does shift *when* a hot function crosses from the interpreter to JIT
mid-execution, so two crossover invariants are each pinned by a test that fails
if the shift mishandles the boundary: a fuel-suspended callee's frame survives
the JIT tier ([fuel-jit-preempt.lisp](../../tests/impl/fuel-jit-preempt.lisp)),
and a value emitted from JIT-compiled code is retained as it escapes into
`fiber.signal`, where the resumer reads it
([region-jit-emit-escape-uaf.lisp](../../tests/impl/region-jit-emit-escape-uaf.lisp)).

## One Cranelift in the dependency graph

That sharing holds only while `wasmtime` and the JIT's `cranelift-*` crates
resolve to the same `cranelift-codegen`. Cargo picks one version per
semver-incompatible requirement, so a JIT held a minor line behind the
Cranelift `wasmtime` carries puts two complete copies of the code generator
into a `--features wasm` build. The copies do not stay independent either:
they share the one `regalloc2` the union of their requirements allows, so the
JIT every default build runs gets its register allocator chosen by the WASM
tier's pin.

The two pins therefore move together. `wasmtime 49` carries Cranelift 0.136,
and `Cargo.toml` pins `cranelift-codegen`, `-frontend`, `-module`, `-jit`, and
`-native` at 0.136 to match. Raising `wasmtime` means raising the JIT's
Cranelift in the same change, which is a code change and not only a manifest
one. Each Cranelift line has changed a builder call the translator makes
([jit.md](jit.md)).
`integration::deps` ([deps.rs](../../tests/integration/deps.rs)) reads
`Cargo.lock` and fails if the graph ever holds two versions of
`cranelift-codegen` or `regalloc2`. It also fails if `wasmtime` resolves below
the release that fixed its newest published advisory. When `cargo audit` names
a newer fix, raise that floor in the same change as the pin.

## See also

- [wasm.md](wasm.md) — the WebAssembly backend
- [jit.md](jit.md) — the Cranelift JIT
