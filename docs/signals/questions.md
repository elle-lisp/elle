# Signal Questions

<!-- audited: 2026-09-07 -->

What the signal design has not settled, and what it has.

## Open Questions

### Signal bit allocation

64 bits (SignalBits is u64): bits 0–17 built-in, bits 18–31
runtime-reserved, bits 32–63 user-defined (up to 32 user signals).

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
  stack. See [fibers.md](fibers.md).

- **Error representation**: Errors are values — by convention a struct
  `{:error :keyword :message "..."}`, but any value works. No `Condition`
  type, no signal hierarchy. Pattern matching on the payload replaces hierarchy
  checks. See [recovery.md](recovery.md).

- **Coroutine pattern**: `yield` works as a special form (emits
  `SIG_YIELD`). A generator fiber is `(fiber/new fn |:yield|)`, resumed
  with `(fiber/resume f val)`. `try`/`catch` is a prelude macro.

- **Signal erasure**: A `ClosureTemplate` carries the code's signal, and a
  `Closure` carries the squelch mask that narrows it — `effective_signal()`
  combines the two. `SignalBits` wraps a `u64`. Acceptable cost.

- **Compound signals**: Functions routinely carry multiple signal bits.
  A function that does I/O and can error has bits `|:error :io|`. The
  compiler infers compound signals by unioning the bits of all callees.
  No bit is privileged: a mask catches a compound signal when the two share
  any bit, so `|:error|` catches `|:error :io|`. The scheduler still sees
  every request it must service, because an I/O request raises `|:io|` and
  carries no `:yield` — so a `|:yield|` mask shares no bit with one and
  cannot swallow it. See [capabilities.md](capabilities.md).

---

## See also

- [Signal index](index.md)
