# Where an import finds its file

<!-- audited: 2026-11-20 -->

How `import/resolve` turns a spec into a path, and how `meta/location` gives each import the directory of its writer.

[modules.md](../modules.md) states what a load does with the path. The
default loader `import/load` asks `import/resolve` for it, and a loader of a
program's own may ask the same function or another.


## Resolution

`(import/resolve spec &opt dir)` returns the absolute, normalized path of the
file that `spec` names. It raises an `:io-error` when `spec` names no file.

Two virtual prefixes come first:

| Prefix | Resolves to | Example |
|--------|-------------|---------|
| `std/X` | `<root>/lib/X.lisp` | `(import "std/portrait")` |
| `plugin/X` | `<root>/target/<profile>/libelle_X.<suffix>` | `(import "plugin/regex")` |

The project root is `--home` (or `ELLE_HOME`), or the first directory above
the elle binary that holds a `Cargo.toml`. A plugin is looked for under the
profile of the running binary (release or debug) first, then under the other.
A prefix whose file does not exist falls through to the search below.

A spec that starts with `./` or `../` names a file relative to `dir`, then
relative to the working directory. Without `dir`, it names a file relative to
the working directory alone. An absolute spec is looked for from the
filesystem root alone. Any other spec is looked for in these directories, in
order:

1. The working directory
2. Each `--path` / `ELLE_PATH` entry (colon-separated)
3. `--home` / `ELLE_HOME`, or the directory of the elle binary

A relative `--path` entry resolves against the working directory. A bare spec
never searches the writer's directory. A file in the working directory
shadows a file of the same name on the path or in home, but never a `std/` or
`plugin/` spec whose file exists.

In each directory, `import/resolve` tries:
- `<dir>/<spec>.lisp`
- `<dir>/<spec>` (as it stands)
- `<dir>/<spec_dir>/libelle_<leaf>.<suffix>` (hierarchical plugin layout)
- `<dir>/libelle_<leaf>.<suffix>` (flat plugin layout)

The suffix is the platform's: `so`, `dylib` or `dll`. `import/resolve` reads
it and the two settings from `vm/config` ([config.md](../config.md)).

```lisp
(def here (path/parent (get (meta/location) :file)))
(def me (path/join here "resolution.md"))
(assert (= "base64.lisp" (path/filename (import/resolve "std/base64"))))
(assert (= me (import/resolve "./resolution.md" here)))
(def from-cwd (string "./" (path/relative me (path/cwd))))
(assert (= me (import/resolve from-cwd))
        "without a directory, a ./ spec names a file in the working directory")
(assert (= me (import/resolve from-cwd "/nonexistent"))
        "a ./ spec that names nothing in dir falls back to the working directory")
(let [[ok? err] (protect (import/resolve "./no-such-module"))]
  (assert (and (not ok?) (= :io-error (get err :error)))
          "a spec that names no file raises :io-error"))
```


## The writer's directory: meta/location

`(meta/location)` is a special form. It returns `{:file :line :col}` for the
form itself, as `(meta/origin f)` does for a closure. `:file` is an absolute
path, or nil for code with no file: the REPL, stdin, `-e`, and a datum that
`eval` compiles. The compiler fixes the value, so it does not depend on who
calls the code.

A form that a macro builds carries the location of the macro call. So the
`meta/location` inside the expansion of `import` names the file that called
`import`, and a relative spec follows the file that wrote it, whatever file
calls the function that holds the import:

```lisp
(def loc (meta/location))
(assert (= "resolution.md" (path/filename (get loc :file))))
(assert (= loc:file (path/absolute loc:file)) "the file is an absolute path")
(let [importer ((import "../../tests/modules/importer"))
      m (importer:load)]
  (assert (= m:test-var 42) "importer.lisp's import resolves beside importer.lisp"))
```
