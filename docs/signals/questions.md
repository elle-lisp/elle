# Signal Questions

## Open Questions

### Signal bit allocation

32 bits: 7 used (0–6) + 9 reserved (7–15) + 16 user-defined (16–31). If
users need more than 16 signal types, bump SignalBits to u64.

### Interaction with the type system

Elle doesn't have a static type system (yet). Signals are the closest thing
to static types. Should they evolve toward a type system, or remain a
separate concern?

### Signal subtyping

Should there be a hierarchy of signal types, or is the flat bitfield
sufficient? Janet uses a flat space. Koka uses a hierarchy. Flat is simpler
and faster. Current implementation: flat.


## Resolved Questions

- **Signal resumption**: Yes. Resume value is pushed onto the child's operand
  stack. See `docs/fibers.md`.

- **Error representation**: Errors are values — by convention a struct
  `{:error :keyword :message "..."}`, but any value works. No `Condition`
  type, no signal hierarchy. Pattern matching on the payload replaces hierarchy
  checks. See the "Error Signalling" section below.

- **Coroutine aliases**: `yield` works as a special form (emits
  `SIG_YIELD`). `make-coroutine` / `coro/resume` are thin wrappers
  around `fiber/new` / `fiber/resume`. `try`/`catch` is a prelude macro.

- **Signal erasure**: Signal bits are stored on the `Closure` struct
  (`SignalBits` = 4 bytes per closure). Acceptable cost.

- **Compound signals**: Functions routinely carry multiple signal bits.
  A function that does I/O and can error has bits `|:error :io|`. The
  compiler infers compound signals by unioning the bits of all callees.
  Compound signals that include `:io` receive special treatment: a fiber
  mask that catches `:error` but not `:io` will *not* catch a compound
  `:error :io` signal. The signal remains uncaught until a handler that
  catches `:io` is reached — the scheduler must see the `:io` bit to
  submit the operation to the backend.

---

## See also

- [Signal index](index.md)
