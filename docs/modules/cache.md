# Modules: the compiled forms a load keeps

<!-- audited: 2026-11-21 -->

The default loader keeps each file's compiled form: an import of the same text compiles nothing, and still evaluates the form.

[modules.md](../modules.md) states what a load does. A compiled form is the
function `import/load-syntax` builds from a module's form before it calls it.
`import/load` keeps the compiled form of each `.lisp` file it loads, under the
file's resolved path, beside the text it read. Every import reads the file
again, so a fiber that withholds `:fs` cannot import it. When the text matches
the kept text, the import compiles nothing. An edited file compiles again, and
its compiled form replaces the kept one. A text that does not compile keeps
nothing.

An import that compiles nothing still evaluates the form. It calls the kept
compiled form in a fiber under the deny set and walks the value, as a first
load does. So each import builds again what the form builds, under the
dynamic bindings where that import stands.

The table of compiled forms is Elle state, held in a parameter the standard
library does not export. `ev/run` binds a new table when none is bound, so a
program has one table, and an `ev/run` inside the program shares it. The other
loaders keep nothing.

Two values built by one compiled form share its code, and each immutable
constant the compiler built for it. So `%identical?` answers true for such a
constant in both:

```lisp
(with-temp-dir dir
  (let [p (path/join dir "counter.lisp")]
    (file/write p "(fn [] (var n 0) (fn [] (assign n (+ n 1)) n))\n")
    (assert (not (%identical? (import p) (import p)))
            "two imports of one file build two values")
    (let [a ((import p))
          b ((import p))]
      (a)
      (a)
      (assert (= 1 (b)) "two imports keep independent state"))
    (file/write p "(fn [] :edited)\n")
    (assert (= :edited ((import p))) "an edited file compiles again")
    (file/write p "{:names [\"a\" \"b\"]}\n")
    (assert (%identical? (get (import p) :names) (get (import p) :names))
            "one constant serves both imports")))
```

[import-cache.lisp](../../tests/lang/import-cache.lisp) holds the tests, and
names the wrong loader each one catches.
