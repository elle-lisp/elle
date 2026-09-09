# What the experiments measured

<!-- audited: 2026-09-08 -->

Six assumptions the image design rests on, each dispatched by an experiment,
with the numbers it produced.

The assumptions were cheap to test and expensive to be wrong about, so they ran
before the foundations landed. [image.md](../image.md) owns the design they
support, [foundations.md](foundations.md) the representation fixes two of them
cleared, and [plan.md](plan.md) the order everything lands in.

1. **Boot-time attribution — dispatched, value proposition confirmed.**
   `--trace=boot,compile` (landed with this design; pinned by
   `tests/integration/trace_boot.rs`) attributes a warm release-build boot
   of a trivial script (~545 ms total): stdlib frontend compile ~494 ms
   (91%), of which region inference ~216 ms, expand ~95 ms, analyze
   ~75 ms, emit ~74 ms, lower ~18 ms; core ~8 ms; prelude, registration,
   and meta build ~2 ms combined; stdlib *execute* ~1.4 ms; ~38 ms of
   process/VM setup and teardown remain. The image removes everything but
   that ~38 ms floor and the 1.4 ms execute — roughly a 10× boot. Region
   inference alone is 44% of the stdlib compile, an independent
   optimization target for the fallback path.
2. **Post-boot heap census — dispatched, sealing confirmed.**
   `--trace=census` (landed with this experiment; pinned by
   `tests/integration/census.rs`, whose sealing net fails the moment an
   unsealed variant enters the boot graph) walks every live object in the
   instance's region store after boot. A warm release boot leaves **402
   regions**, **640 objects**, 3.81 MiB of committed region pages, and
   1.25 MiB of body payload:

   | Tag | Count | Bytes | Heap-ptr slots | Slices |
   |-----|-------|-------|----------------|--------|
   | ClosureTemplate | 202 | 1,227,041 | 13 | 202 |
   | Closure | 201 | 36,064 | 826 | 148 |
   | CaptureCell | 220 | 31,680 | 215 | 0 |
   | LStruct | 4 | 10,400 | 198 | 0 |
   | Parameter | 8 | 1,024 | 3 | 0 |
   | External | 3 | 384 | 0 | 0 |
   | LStructMut | 2 | 400 | 3 | 0 |

   The boot residue is code, not data: no pair, string, or array survives
   to the post-boot heap. The 220 capture cells are the snapping set — the
   static scan found zero top-level `assign`s in stdlib.lisp, so every
   cell snaps. The unsealed leaves are exactly the reconstruction stream's
   two classes: the three stdio-port `External`s and the two default
   traitsets ([image.md](../image.md) § "Process-owned resources reconstruct
   in place"); nothing else in the graph is refused, so the boot image is
   dumpable. Relocation load: 1,258 heap-pointer slots + 350 `RegionSlice`
   ptrs + 684 primitive slots ≈ 2,300 relocation entries.

   Most of the payload is source locations. The template foundation moved a
   code object's data into region pages, so what the census now measures
   includes the location tables — 16 bytes an entry over an interned file
   table, where the `HashMap<usize, SourceLoc>` they replaced held a `String`
   per entry on the Rust heap, invisible here and several times larger. Every
   boot closure already referenced a region-resident template object, so that
   foundation rewrote the representation rather than the sharing structure:
   the census's `shared-templates` counter, which measured how many closures
   still held an `Rc` blueprint, retired with that variant.
3. **The store spike — dispatched, mechanism proven.** `src/image` dumps
   and hydrates data-only graphs (pairs, strings, bytes, arrays, floats,
   portable immediates) end to end, pinned by `tests/integration/image.rs`
   against [plan.md](plan.md) § Test plan: round-trip equality in a fresh
   heap, sharing preservation, corrupted-fingerprint fallback with no leaked
   region or mapping, double hydration with independent address sets,
   rename-over-a-live-mapping, teardown to baseline, and file-backed release
   as `munmap` (never cached — also pinned at the pool in `pagepool/tests.rs`).
   The pool interplay reduced to one field: `MmapPage` carries a `file_backed`
   flag the release path checks. Two findings: relocation slots and targets
   collapse to region-relative offsets (recorded in [format.md](format.md)),
   and raw object-slot bytes are not byte-deterministic under `repr(Rust)`
   padding (recorded in [image.md](../image.md) § Dumping; resolved by item
   6's extent copy). The three the spike left open have since landed with the
   **store** milestone: the `(fd, offset)` input form with an anonymous
   memory file for byte sources, the verifier's object walk, and the
   scrub/guardfree pins over a hydrated region. A fourth finding came from
   the guardfree pin: `mprotect` is not a way to ask whether a page is
   mapped, because asking makes it inaccessible — `msync` is.
4. **Symbol-identity scout — dispatched, migration confirmed cheap.** The
   audit classified all 221 `SymbolId` sites, and a throwaway prototype —
   `SymbolId(u64)` minted as the FNV-1a name hash, `SymbolTable` reduced
   to a hash→name registry with the keyword collision panic — ran the full
   suites. Site classes and the cost of each:

   | Site class | Sites | Migration cost |
   |------------|-------|----------------|
   | Opaque keys and pass-throughs (`PrimitiveMeta`, classification maps, inline/dispatch registries, JIT `scc_peers`, binding arenas) | ~170 | none — already hash-keyed |
   | Width seams: `SymbolId(u32)`; `Value::symbol(u32)`; the truncating `as_symbol() → u32`; `Bytecode::add_symbol(u32)`; `SendValue::Symbol.id` (dead on receive); `errors.rs` parses `SymbolId(N)` as `u32`; 66 `HashMap<u32, String>` name maps | ~75 | mechanical widening — the whole prototype is 39 files, ±110 lines, and compiles clean beyond these seams |
   | Dense indexing beyond `SymbolTable` | 1 | `jit/group.rs` `globals[sym.0 as usize]` — dead code with test-only callers; there is no VM globals table (the letrec model has no `LoadGlobal`), so no live density assumption exists |
   | Bytecode operands carrying a symbol id | 0 | none to audit: symbols reach bytecode only as constant-pool `Value`s (u16 pool index) and `ConstTemplate`s, which already encode symbols by name |
   | Raw-id comparators (`Value::Ord` rank-3 arm, `TableKey::Ord` symbol arm) | 2 | sort order flips to hash order coherently; sorted structs, sets, and their binary searches stay correct because build and probe share the comparator |
   | Sentinel `SYNTHETIC = u32::MAX` | 1 production read | becomes a reserved `u64::MAX`; the binding's existing `is_synthetic` flag could replace it outright |

   Measured fallout: Rust suites green except two tests that pin the
   property being removed (the sequential-mint assertion, and a
   different-ids-across-two-tables setup assert). The full smoke corpus —
   2,264 files, VM and JIT — passes with **zero expectation churn**: no
   Elle test observes symbol sort or print order. `(environment)` is the
   one producer of symbol-keyed structs, and nothing pins its key order.
   Keyword-keyed containers sort by name string and are unaffected. Boot
   is unharmed: quiet-machine stdlib-compile is not slower under hash
   interning. Deleted by the migration: the five `symbol_names` maps and
   their threading, `all_names()`, `send`'s by-name symbol re-intern,
   `intern_primitive_names` and its five call sites, and the `CompileCtx`
   registration-order invariant (including the bullet in
   [pipeline.md](../../pipeline.md)). The audit also found two live
   cross-table id holes that stable ids close: `send` ships
   `LirConst::Symbol` inside the live `LirFunction` verbatim, so worker-side
   JIT re-emission pools sender-space ids; and `TableKey::Symbol` keys inside
   sent structs cross untranslated. The symbol milestone must land regression
   tests for both.
5. **Expander mutation parity — dispatched, parity exceeded.** A throwaway
   prototype node ran the expander's hot operations head to head against
   the Rust-heap tree, allocating through the real region store
   (`FiberHeap::alloc_region_slice_in_region`). The node is 56 bytes to
   `Syntax`'s 112: kind tag, packed span over an interned file id, scope
   set inline (capacity 4 plus an overflow slice), string payloads and
   child slices as `RegionSlice`, symbols as stable hashes (the symbol
   foundation lands first). Corpus: the parsed prelude + stdlib trees
   (230 forms, 13,332 nodes), 20 rounds per op, in-place walks mutating
   uniquely owned trees through the child slices:

   | Per node | Rust-heap tree | Region tree |
   |----------|----------------|-------------|
   | stamp-copy (macro-arg clone + add-scope walk) | 176 ns | 37 ns |
   | hygiene flip walk, in place | 40 ns | 17 ns |
   | file-scope add walk, in place | 31 ns | 13 ns |
   | build + drop (the `from_value` shape) | 73 ns | 17 ns |
   | teardown | 31 ns | 7 ns |

   Region mint + free measures ~6 ns, so per-expansion transient regions
   are noise. The no-inline-capacity fallback — regrow the scope slice on
   every add — costs 8 ns per visit, so scope storage is not a parity risk
   in either form. The op mix is measured, not assumed: counters on
   `Syntax::clone`, the constructors, `map_scope_recursive`, and the
   converters (dumped at the `--trace=compile` expand/analyze marks)
   showed the stdlib expand phase (84.5 ms warm release) deep-clones
   **464,089** nodes against 46,746 built and 24,718 from `from_value`,
   across 1,532 expansions; analysis clones another 143,298. A 300-defn
   macro-heavy user file amplified the same shape: 992,170 clones in a
   206 ms expand, ~254 clones per expansion — a large share being the
   per-call `MacroDef` template deep clone, which pointer-shared immutable
   region trees delete outright. perf agrees: `Syntax::clone` +
   `drop_in_place<Syntax>` is ~10% of the whole boot. Scope sets are tiny
   everywhere: every one of the ~430k measured scope ops ended with ≤3
   scopes. Counts × per-op deltas put tree ops near half of an
   expansion-heavy expand phase and ~4× cheaper in regions, so the
   migration is projected to make expansion-heavy compiles roughly a
   third *faster* — and even a fully immutable working tree meets the
   bar, since a full stamp-copy (37 ns) undercuts the Rust tree's
   in-place walk (40 ns). The boundary-only split is dead; no measured
   deal-breaker exists. One condition binds the migration: keep in-place
   mutation legal on uniquely owned working trees (stamped copies and
   conversion results — the ownership discipline the hygiene flip already
   relies on). To redo: counter patch at the sites above, plus a
   `#[cfg(test)]` bench under `src/syntax/expand/` building the prototype
   node from `read_syntax_all` output and running stamp/flip/add/build/
   teardown against `stamp_scope`/`flip_scope_recursive`. The shipped node
   differs from the prototype in two ways [syntax.md](../syntax.md) argues
   for: it carries a symbol's spelling as a region string rather than a
   hash, and its scope set has no inline capacity (the prototype measured
   the difference at 8 ns per visit). It is 64 bytes, not 56.
6. **Fingerprint strength — dispatched, probes landed.** Size/align probes
   do not pin field offsets; the fingerprint now records, per dumpable
   variant, the discriminant byte and every leaf field's offset and length
   (`src/image/layout.rs`), so the two-stage embed build cannot pass the
   fingerprint with a shifted layout. Mechanism finding: `offset_of!`
   cannot name an enum variant's field on stable Rust (E0658,
   rust-lang/rust#120141), so the probes construct one exemplar per variant
   and measure each field's address against the object's base, with
   `offset_of!` covering the nested structs (`Pair`, `Value`,
   `RegionSlice`). Measured layout (rustc 1.95, x86-64): `HeapObject` is
   128 bytes, align 8. The by-value `ClosureTemplate` variant used to set
   that size at 288 bytes, making a `Float` slot ~95% padding; the template
   foundation reduced the variant to a payload slice plus a blueprint
   pointer. The discriminant is one byte at offset 0 (declaration index
   plus 3) with bytes 1–7 zero; every probed variant places its payload
   field at 8 and `traits` at 24 (`Pair` nests its whole struct at 8).
   `RegionSlice`'s `u32` len leaves interior padding at bytes 12–16 of the
   field, which is why extents are recorded per leaf field, never per
   variant field. The probes verify themselves on first use — distinct
   discriminant bytes, zero upper discriminant bytes, disjoint in-bounds
   extents, and a canonicalize-then-read-back check per variant — and
   panic on violation, so a rustc that moves the tag or reorders fields
   fails loudly before any image is written or trusted. The unlock
   ([image.md](../image.md) § Dumping): the dumper assembles object slots
   from the extents, dumps are byte-identical whole files, and the
   determinism pin asserts whole-file equality with a poisoned-padding
   counter-factual.
