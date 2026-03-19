# plugins/clap

CLI argument parsing plugin for Elle via the `clap` crate.

## Responsibility

Provides a single `clap/parse` primitive that accepts a declarative command
spec struct and an argv list/array of strings, then returns a struct of
parsed values. No opaque objects — spec in, struct out.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `clap/parse` | Exact(2) | Parse argv against a command spec; returns a struct of parsed values |

## Command spec keys

Top-level spec: `:name` (required string), `:about` (optional string),
`:version` (optional string), `:args` (optional array of arg specs),
`:commands` (optional array of sub-command specs).

Arg spec: `:name` (required string), `:long` (optional string), `:short`
(optional single-char string), `:help` (optional string), `:action`
(optional keyword — `:set`, `:flag`, `:count`, `:append`; default `:set`),
`:required` (optional bool), `:default` (optional string), `:value`
(optional string — metavar for help display).

An arg with neither `:long` nor `:short` is positional.

## Parse result

A struct with one key per arg `:name` (as a keyword). Actions determine the
value type: `:set` → string or nil, `:flag` → bool, `:count` → int,
`:append` → array of strings. When subcommands are defined, two extra keys
appear: `:command` (string or nil) and `:command-args` (struct or nil).

## Error kinds

| Condition | Error kind |
|-----------|-----------|
| `spec` not a struct | `type-error` |
| `argv` not a list or array | `type-error` |
| `argv` element not a string | `type-error` |
| `:name` missing from spec | `clap-error` |
| `:name` not a string | `type-error` |
| Unknown `:action` keyword | `clap-error` |
| Arg spec missing `:name` | `clap-error` |
| `:short` not a single char | `clap-error` |
| Arg named `"command"`/`"command-args"` with `:commands` present | `clap-error` |
| clap parse failure | `clap-error` |

## Building

```bash
cargo build --release -p elle-clap
# Output: target/release/libelle_clap.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_clap.so"))
(def parse-fn (get plugin :parse))
(parse-fn {:name "app" :args [{:name "v" :long "verbose" :action :flag}]} ["--verbose"])
#=> {:v true}
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
