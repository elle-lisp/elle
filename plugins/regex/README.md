# elle-regex

A regex plugin for Elle, wrapping the Rust `regex` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_regex.so` (or `target/release/libelle_regex.so`).

## Usage

```lisp
(import-file "path/to/libelle_regex.so")

(def re (regex/compile "\\d+"))
(regex/match? re "abc123")       ;; => true
(regex/find re "abc123def")      ;; => {:match "123" :start 3 :end 6}
(regex/find-all re "a1b2c3")     ;; => ({:match "1" ...} {:match "2" ...} {:match "3" ...})

(def date-re (regex/compile "(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})"))
(regex/captures date-re "2024-01-15")
;; => {:0 "2024-01-15" :1 "2024" :2 "01" :3 "15" :year "2024" :month "01" :day "15"}
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `regex/compile` | pattern | compiled regex |
| `regex/match?` | regex, text | boolean |
| `regex/find` | regex, text | `{:match :start :end}` or nil |
| `regex/find-all` | regex, text | list of match structs |
| `regex/captures` | regex, text | struct with numbered/named groups, or nil |
