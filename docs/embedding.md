# Embedding

Elle can be embedded as a scripting engine in Rust or C programs. The host
creates a runtime, optionally registers custom primitives, compiles and
executes Elle code, and extracts results.

## Concepts

**Elle as a library.** The `elle` crate exposes everything needed to embed
a full Elle runtime: VM, SymbolTable, compilation pipeline, and stdlib.
The host owns the VM and controls its lifetime.

**Scheduler cooperation.** All Elle I/O is async — it requires a scheduler.
`execute_scheduled` wraps user code in `ev/run` automatically. For hosts
that need to interleave their own work, the step-based scheduler provides
cooperative control.

**The step model.** `make-async-scheduler` returns a struct with a `:step`
entry. Each call to `:step` drains runnable fibers, processes one batch of
I/O completions, and returns `:done` or `:pending`. The host can interleave
its own event loop between steps.

## Rust embedding

The init sequence:

1. `VM::new()` + `SymbolTable::new()` — create runtime state
2. `register_primitives(&mut vm, &mut symbols)` — install builtins
3. `set_vm_context` + `set_symbol_table` — set thread-local context
4. `init_stdlib(&mut vm, &mut symbols)` — load stdlib
5. Register custom primitives via `PrimitiveDef` + `register_repl_binding`
6. `compile_file(source, &mut symbols, filename)` — compile to bytecode
7. `vm.execute_scheduled(&bytecode, &symbols)` — run under async scheduler
8. `set_vm_context(null_mut())` — clear context when done

Custom primitives are `fn(&[Value]) -> (SignalBits, Value)` wrapped in a
`PrimitiveDef` struct with metadata (name, arity, signal, docs). Register
with `register_repl_binding` so the compiler sees them.

See `demos/embedding/src/main.rs` for the complete Rust host demo.

## C embedding

The `elle-embed` cdylib provides a C-ABI surface:

| Function | Purpose |
|----------|---------|
| `elle_init()` | Create runtime (returns opaque pointer) |
| `elle_destroy(ctx)` | Cleanup |
| `elle_eval(ctx, src, len)` | Compile + execute (0=ok, -1=error) |
| `elle_result_int(ctx, &out)` | Get result as integer |
| `elle_register_prim(ctx, ...)` | Register host primitive |

Link against `libelle_embed.so` with `-lelle_embed`. See
`demos/embedding/host.c` and `demos/embedding/include/elle.h`.

## Event loop cooperation

For hosts with their own event loop, the step-based scheduler provides
cooperative interleaving:

```lisp
(let [sched (make-async-scheduler)
      f (fiber/new (fn [] (+ 1 2 3)) |:yield|)]
  ((get sched :spawn) f)
  (def @status :pending)
  (while (= status :pending)
    (assign status ((get sched :step) 0)))
  (assert (= status :done))
  (assert (= (fiber/value f) 6)))
```

The `ev/step` function wraps this for use inside an existing event loop:

```lisp
(let [sched (make-async-scheduler)]
  (parameterize ((*scheduler* sched)
                 (*spawn* (get sched :spawn))
                 (*shutdown* (get sched :shutdown))
                 (*io-backend* (get sched :backend)))
    (ev/spawn (fn [] (+ 10 20 30)))
    (def @status :pending)
    (while (= status :pending)
      (assign status (ev/step)))
    (assert (= status :done))))
```

## Step API reference

- `(ev/step)` — non-blocking step (timeout 0ms), returns `:done` or `:pending`
- `(ev/step timeout-ms)` — step with timeout, blocks up to `timeout-ms` for I/O
- `((get sched :step) timeout-ms)` — direct scheduler step (no `*scheduler*` required)

The `:pump` entry is defined in terms of `:step`:

```lisp
# pump = step with infinite timeout until done
(def @status :pending)
(while (= status :pending)
  (assign status (ev/step (- 0 1))))
```
