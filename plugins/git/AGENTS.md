# elle-git Plugin — Agent Reference

## Summary

`elle-git` exposes `git2` crate functionality as Elle primitives. It wraps
`git2::Repository` as an `External` value and provides 32 primitives across
repository lifecycle, branches, commits, staging, diff, tags, remotes, and
config.

## Module Layout

```
src/
├── lib.rs       — plugin entry point + PRIMITIVES table
├── helpers.rs   — shared helpers: get_repo, get_string, git_err, opts_get_*,
│                  diff helpers (DiffState, diff_to_value, etc.), make_signature
├── repo.rs      — git/open, git/init, git/clone, git/path, git/workdir,
│                  git/bare?, git/state, git/head, git/resolve
├── branches.rs  — git/branches, git/branch-create, git/branch-delete, git/checkout
├── commits.rs   — git/commit-info, git/log, git/commit
├── staging.rs   — git/status, git/add, git/remove, git/add-all
├── diff.rs      — git/diff, git/diff-patch, git/show
├── tags.rs      — git/tags, git/tag-create, git/tag-delete
├── remotes.rs   — git/remotes, git/remote-info, git/fetch, git/push
└── config.rs    — git/config-get, git/config-set
```

## External Type

`"git/repo"` — wraps `git2::Repository` directly (no `RefCell` needed because
all methods we call on repo either take `&self` or return owned values we
mutate locally within the call).

## Key Invariants

1. **`ctx.init_keywords()` must be first** in `elle_plugin_init` — each cdylib
   has its own keyword name table; init_keywords routes to the host's table.

2. **`Repository` is not `RefCell`-wrapped** — `repo.index()` returns an owned
   `Index` we mutate and drop within the same call. `repo.find_remote()` returns
   an owned `Remote` we mutate as `mut remote`. No shared mutable state.

3. **OIDs are hex strings** — all OID values returned to Elle are 40-char hex
   strings via `.to_string()`.

4. **Timestamps are i64 epoch seconds** — from `Signature::when().seconds()`.

5. **Lists vs arrays** — multi-item top-level results use `elle::list()`.
   Nested fields inside structs (`:parents`, `:files`, `:hunks`, `:lines`) use
   `Value::array()`.

6. **Error kinds** — `"git-error"` for git2 operation failures, `"type-error"`
   for wrong argument types.

## opts Struct Access Pattern

Use the helpers in `helpers.rs` — never call `.as_struct()` directly in
primitives:

```rust
let opts_val = if args.len() >= 2 { Some(args[1]) } else { None };
let force = opts_get_bool(opts_val, "force");
let limit = opts_get_int(opts_val, "limit");
let from_ref = opts_get_string(opts_val, "from");
```

## Adding a New Primitive

1. Add implementation function in the appropriate module.
2. Add `PrimitiveDef` entry in `lib.rs` PRIMITIVES table.
3. Export the function as `pub fn` so `lib.rs` can reference it.
4. Add test coverage in `tests/git.elle`.
