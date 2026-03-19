# plugins/semver

Semantic version parsing, comparison, and manipulation via the `semver` crate.

## Responsibility

Provides 5 primitives for working with semantic versions (semver 2.0.0):
parsing version strings into structured data, validating version strings,
comparing versions, checking version requirements, and incrementing version
parts.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `semver/parse` | Exact(1) | Parse a version string into `{:major :minor :patch :pre :build}` |
| `semver/valid?` | Exact(1) | Return true if the string is a valid semver version |
| `semver/compare` | Exact(2) | Compare two version strings, returning -1, 0, or 1 |
| `semver/satisfies?` | Exact(2) | Check if a version satisfies a requirement string |
| `semver/increment` | Exact(2) | Increment a version part (`:major`, `:minor`, or `:patch`) |

## Implementation

Uses the `semver` crate (version 1.x) for parsing, comparison, and requirement matching.

`semver/parse` returns an immutable struct with:
- `:major`, `:minor`, `:patch` as integers
- `:pre` and `:build` as strings (empty string when absent, never nil)

`semver/increment` takes a keyword as the second argument (`:major`, `:minor`,
or `:patch`). It clears lower parts and pre-release/build metadata when
incrementing. `init_keywords()` is called in `elle_plugin_init` to route
keyword lookups to the host's name table — required because the plugin receives
keyword values created in the host.

## Error kinds

| Condition | Error kind |
|-----------|-----------|
| Invalid version string | `semver-error` |
| Invalid requirement string | `semver-error` |
| Unknown increment part keyword | `semver-error` |
| Wrong argument type | `type-error` |
| Wrong argument count | `arity-error` |

## Building

```bash
cargo build --release -p elle-semver
# Output: target/release/libelle_semver.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_semver.so"))
(def parse-fn (get plugin :parse))
(parse-fn "1.2.3")
#=> {:major 1 :minor 2 :patch 3 :pre "" :build ""}
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
