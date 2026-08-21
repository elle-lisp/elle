# Rich errors — one region-coherent struct routine + `rich_error!`

Implementation-facing. An Elle error is an ordinary struct
`{:error <kind-keyword> :message "<text>" …extra-fields}` — there is no
exception machinery (signals carry the value; see [vm.md](../vm.md)). "Rich" means
the struct carries fields beyond `:error`/`:message` (`:path`, `:value`,
`:tier`, …). This page defines the single construction surface and the
region-coherence invariant it guarantees.

## Why errors need their own care

An error value is a heap struct, so it is born in a region (Rule 3,
[ctx.md](ctx.md)). Its **field values are themselves heap
allocations** — a `:path` string, a `:message` string — and they must live in
the *same* region as the struct that points at them. If a field value were built
in a different region, freeing the error's region would either strand that
field's region (leak) or, worse, the field's region could be freed first, leaving
the error pointing at recycled pages (UAF). So the construction surface must
guarantee: **message and every field are born in one region, the error's own.**

This is the invariant a naive `error(kind, msg)` two-arg call cannot express, and
that a "build the field `Value`s first, pass them in a slice" API gets wrong the
moment a field value is a freshly-built string allocated *before* the error's
region exists.

## The routine: `error_extra`

`error_extra` is the one routine that builds the whole error in one region —
message string, the `:error`/`:message` pairs, and every extra field — so the
co-location invariant holds by construction:

```rust
// NativeCtx — the error is born in the ctx's own region (the call's result
// region; the ctx owns it and exposes no getter):
fn error_extra(&self, kind: &str, msg: impl Into<String>, extra: &[(&str, Value)]) -> Value;

// VM — the error is born in a fresh region minted by a `NativeCtx::new(self.heap())`,
// freed value-based by the consumer's DecrefValueRegion. Same name as the
// ctx method so `rich_error!` is uniform whether the source token is `ctx` or
// the VM (`self`/`vm`).
fn error_extra(&mut self, kind: &str, msg: impl Into<String>, extra: &[(&str, Value)]) -> Value;
```

The `extra` values must be **born in the same region** — built through the same
region source (`ctx.string(x)`), or region-free (keywords/ints are immediates; a
pass-through `Value` already owns its region and is incref'd into the error's
region by `alloc`'s content scan, exactly as the `:value` field of a match error).
The discipline that keeps this true: **build string fields through the same
region source you build the error with** — `ctx.string(path)` for a `ctx` error.

The 2-arg `error(kind, msg)` / `escaping_error` / `set_error` are field-less
sugar (`error_extra` with `&[]`); keep them for the common case.

## The macro: `rich_error!`

`rich_error!` is the call-site sugar over `error_extra`: it DRYs the
`(SIG_ERROR, …)` tuple, the `:error <kind>` field, and the slice-of-pairs, and
takes `field = value` pairs for extension. It is *only* sugar — every value it
places is still written by the caller, so no region is ever hidden.

```rust
return rich_error!(ctx, "io-error", format!("slurp '{path}': {e}"),
                   path = ctx.string(path));
// expands to:
//   (SIG_ERROR, ctx.error_extra("io-error", format!(...), &[("path", ctx.string(path))]))

// VM scope (fields are keywords / pass-throughs — region-coherent):
return rich_error!(self, "tier-rejected", message,
                   tier = Value::keyword(tier), reason = Value::keyword("ineligible"));

// field-less still goes through the 2-arg sugar, not the macro:
return (SIG_ERROR, ctx.error("type-error", msg));
```

Notes:

- The first token is the **region source** (`ctx` or `self`/`vm`), explicit at
  every call site so the error and its fields share one region.
- String fields are written `name = ctx.string(x)` so the value is born in the
  error's region. The macro never calls `string` for you on a field (only on the
  `message`, via `error_extra`), so it can't misplace a region.
- Field names are identifiers (`path`, `tier`, `reason`, `value`, `code`, …),
  stamped as keywords via `stringify!`.

## What becomes unrepresentable

| Defect | Why it cannot be written |
|---|---|
| An error field value born in a foreign region | message + fields built in one region by `error_extra`; string fields written through the same region source |
| A rich error that drops its fields through a 2-arg surface | `rich_error!` carries arbitrary `field = value` pairs |
| `(SIG_ERROR, …)` / `:error <kind>` boilerplate drift | the macro is the single expansion |

## Gate

`--trace=guardfree` over the error/region suites is the UAF oracle for the
co-location invariant; a field value stranded in a foreign region surfaces as a
stale deref at free.
