# lib

Reusable Elle modules maintained in this repository. Not part of the core
language; not examples. Libraries that user code can load on demand.

## What belongs here

Modules providing a self-contained, reusable capability (HTTP, JSON, CSV,
etc.) with no coupling to examples or tests. Core language features go in
`stdlib.lisp`. One-off demonstrations go in `examples/`.

## Loading convention

Load a module with `import-file` and a relative path:

```lisp
(def http ((import-file "./lib/http.lisp")))
(http:get "http://example.com/")
```

No auto-loading. Users explicitly import what they need. The module file
returns a closure; calling it produces the exports struct.

## Modules

| File | Purpose |
|------|---------|
| `http.lisp` | HTTP/1.1 client and server |

## File size target

~500 lines per file. If a module exceeds this, split into a subdirectory:
`lib/http/parse.lisp` + `lib/http/module.lisp`.
