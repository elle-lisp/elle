# Signal Questions

## Open Questions

### Compound signals

Can a function emit multiple signal bits simultaneously? Current position:
probably not (signals are suspension points), but the representation supports
it. Revisit if we find a use case.

### Signal bit allocation

32 bits: 7 used (0–6) + 9 reserved (7–15) + 16 user-defined (16–31). If
users need more than 16 signal types, bump SignalBits to u64.

### Dynamic signal checking overhead

Checking `closure.signals & ~bound == 0` at every call boundary with a
signal bound — is this too expensive? It's one AND + one branch. Probably
fine, but worth measuring.

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
  around `fiber/new` / `fiber/resume`. `try`/`catch` macro is blocked on
  macro system work.

- **Signal erasure**: Signal bits are stored on the `Closure` struct (one
  `Signal` value = 8 bytes). Acceptable cost.

---

## See also

- [Signal index](index.md)
