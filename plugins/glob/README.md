# elle-glob

Filesystem globbing plugin for Elle.

## Features

- Match paths against glob patterns
- Expand glob patterns to matching paths
- Support for standard glob syntax: `*`, `?`, `[...]`, `**`

## Building

```bash
cargo build --release
```

The compiled plugin will be at `target/release/libelle_glob.so`.

## Usage

Load the plugin in Elle:

```janet
(import "target/release/libelle_glob.so")
```

### glob/match

Test if a path matches a glob pattern:

```janet
(glob/match "*.lisp" "hello.lisp")      ; => true
(glob/match "src/**/*.rs" "src/vm/mod.rs") ; => true
(glob/match "*.txt" "hello.lisp")       ; => false
```

### glob/glob

Expand a glob pattern to matching paths:

```janet
(glob/glob "examples/*.lisp")
; => #array["examples/closures.lisp" "examples/recursion.lisp" ...]

(glob/glob "src/**/*.rs")
; => #array["src/lib.rs" "src/main.rs" "src/vm/mod.rs" ...]
```

## Glob syntax

- `*` — matches any sequence of characters except `/`
- `?` — matches any single character except `/`
- `[abc]` — matches any character in the set
- `[!abc]` — matches any character not in the set
- `**` — matches any number of directories (including zero)

## Dependencies

- `glob` — filesystem pattern matching
- `elle` — Elle runtime and value types

## License

Same as Elle.
