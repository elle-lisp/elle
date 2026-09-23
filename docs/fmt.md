# elle fmt

<!-- audited: 2026-09-23 -->

Opinionated code formatter for Elle. One canonical style.

## Usage

```sh
elle fmt [OPTIONS] <file...>     # format files in place
elle fmt --check <file...>       # check mode (exit 1 if changes needed)
elle fmt < input.lisp            # format stdin to stdout
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--check` | off | Don't write; list files that need formatting, enforce column limits |
| `--no-epoch` | off | Skip the epoch tag and migration, for a fragment of a file |
| `--plm`, `--preserve-left-margin` | off | Strip the common left margin, format, then put it back |
| `--line-length=N` | 80 | Target line width |
| `--indent-width=N` | 2 | Spaces per indent level |

### The epoch tag

Before it formats, `elle fmt` runs the same epoch migration as `elle rewrite`
([epochs.md](epochs.md)). A file with no `(elle/epoch N)` form gains
`(elle/epoch 12)`, the current epoch, as its first line, and a file tagged
with an older epoch is migrated and retagged. `--no-epoch` skips both, which
is what a fragment pasted from a larger file needs.

`elle fmt` reads every file as s-expression source, whatever its extension. Do
not run it on a `.lua` or `.py` file: it writes the file back as one token per
line under an epoch tag, and exits 0.

### Column enforcement (--check only)

- **Warning at column 60** (three quarters of `--line-length`): an opening
  delimiter past it suggests lifting a lambda or refactoring. A warning does
  not change the exit status.
- **Error at column 80** (`--line-length`): an opening delimiter past it means
  too much nesting; `--check` exits 1.

The check runs on files, not on stdin. It counts CODE delimiters only. A `(`,
`[`, or `{` inside a string literal or a comment is text, not nesting, so it is
skipped — the formatter places those characters itself and must not then
report its own output. Escapes count: the `"` in `"a \" b"` does not close the
string.

## Rule set

Every example below is the output of `elle fmt --no-epoch`, and the build runs
the examples as one program. The build does not run the formatter over them
again, so an edit to an example must go through `elle fmt` first.

### General principles

- **Idempotent.** `format(format(x)) == format(x)`, always.
- **2-space indent.** Every nesting level adds 2 spaces.
- **Trailing newline.** Output always ends with `\n`, and no line ends in
  spaces.
- **Shebang preserved.** `#!/usr/bin/env elle` stays on line 1.
- **Comments preserved.** Inline comments stay inline; block comments
  stay on their own line.

Several rules below call a form *trivial*: at most two levels of nested lists
or collections below it. `(push seen x)` is trivial;
`(begin (push seen (string "x" x)) x)` is not.

### Definitions

`defn`, `defmacro`: header on the first line, body always breaks, +2.

```lisp
(defn fib [n]
  (if (< n 2)
    n
    (+ (fib (- n 1)) (fib (- n 2)))))
(assert (= (fib 10) 55))
```

`def`: inline if it fits, otherwise the value moves to the next line, +2.

```lisp
(def answer 42)
(def greeting
  (string "a value too long for the line its name starts, " "so it moves down"))
```

### Bindings

`let`, `let*`, `letrec`: one binding pair per line, always, names aligned to
the column after `[`. The body always breaks, +2.

```lisp
(let [x 5
      y 10]
  (assert (= (+ x y) 15)))
```

### Conditionals

`if`: with trivial branches, inline if it fits; otherwise the test stays
beside `if` and the branches break, +2. A branch that is not trivial breaks
every branch onto its own line, +2 from `(if`.

```lisp
(defn magnitude [x]
  (if (> x 0) x (- x)))
(def seen @[])
(defn classify [x]
  (if (> x 0)
    (begin
      (push seen (string "positive " x))
      :positive)
    (begin
      (push seen (string "other " x))
      :other)))
(assert (= (magnitude -3) 3))
(assert (= (classify -1) :other))
```

`cond`: always breaks. Each test and body pair takes a line; a trivial body
stays beside its test, and any other body breaks, +2 from the test. A trailing
unpaired element is the default and stands alone.

```lisp
(defn sign [x]
  (cond
    (< x 0) :negative
    (= x 0) :zero
    :positive))
(defn sign-noted [x]
  (cond
    (< x 0)
      (begin
        (push seen (string "negative " x))
        :negative)
    :non-negative))
(assert (= (sign 0) :zero))
(assert (= (sign-noted -2) :negative))
```

`match`: the matched expression stays beside `match`; the pairs follow the
`cond` layout.

```lisp
(defn name-of [n]
  (match n
    1 "one"
    2 "two"
    _ "other"))
(assert (= (name-of 2) "two"))
```

`case`: the same pair layout as `cond`.

```lisp
(defn next-step [status]
  (case status
    :ok :done
    :error
      (begin
        (push seen (string "retrying " status))
        :retry)))
(assert (= (next-step :error) :retry))
```

### Loops and control

`when`, `unless`: a single trivial body goes inline if it fits; any other body
breaks, +2, with the test beside the head.

```lisp
(when (> answer 0)
  (push seen answer)
  answer)
```

`while`: a single body goes inline if it fits; several bodies break.

`each`: the header (`each item [in] collection`) always stays on one line, and
the body always breaks, +2. The `in` keyword is optional.

```lisp
(each item in [1 2 3]
  (push seen item))
(each item [4 5]
  (push seen item))
```

`forever`: a single body goes inline if it fits; several bodies break like
`begin`. Neither function below is called.

```lisp
(defn sleep-forever []
  (forever (ev/sleep 60)))
(defn tick-forever []
  (forever
    (ev/sleep 1)
    (println "tick")))
```

`begin`, `do`, `defer`: always break, body +2.

`block`: like `begin`, with the `:name` on the `block` line.

```lisp
(def found
  (block :search
    (each n in [3 8 11]
      (when (> n 5) (break :search n)))
    nil))
(assert (= found 8))
```

`try`, `protect`: a single body goes inline if it fits; a body followed by a
`catch` or `finally` breaks, +2.

### Functions

`fn`: a single body goes inline if it fits; several bodies always break. The
body indents from the column of `(fn`, so a lambda inside a binding lines up
under its own head.

```lisp
(def inc1 (fn [x] (+ x 1)))
(def total
  (letrec [walk (fn [i acc]
                  (if (>= i 10)
                    acc
                    (walk (+ i 1) (+ acc i (length "seven chars")))))]
    (walk 0 0)))
(assert (= (inc1 1) 2))
(assert (= total 155))
```

### Generic calls

A call inlines if it fits. Otherwise the first argument stays beside the head,
and the rest align to the first argument's column, filling each line before
they break to the next. A keyword and the value after it stay together.

```lisp
(def sentence
  (string "the first argument stays beside the head, "
          "and the arguments that follow " "align under it"))
```

### Logical operators

`and`, `or`, `not`, `emit`: the generic call layout.

```lisp
(def ok?
  (and (nil? (get {:a 1} :b)) (or (= answer 42) (not (empty? seen)))
       (> (length seen) 0)))
(assert ok?)
```

### Threading macros

`->`, `->>`, `some->`, `some->>`: always break. The value stays on the first
line, and the steps align with it.

```lisp
(def evens
  (->> [1 2 3 4 5 6]
       (map inc)
       (filter even?)
       (take 2)))
(assert (= evens (list 2 4)))
```

### Parameterize

Each binding on its own line, aligned to the column after `(parameterize (`.
The body breaks, +2.

```lisp
(def *depth* (make-parameter 0))
(def *label* (make-parameter "outer"))
(parameterize ((*depth* 1)
               (*label* "inner"))
  (assert (= (*depth*) 1)))
```

### Collections

Arrays, sets, and bytes: inline if they fit; otherwise the elements fill each
line and align to the column after the opening delimiter. Structs keep each key
with its value: the whole struct goes inline, or every pair takes a line of its
own.

```lisp
(def small [1 2 3])
(def failure
  {:error :type-error
   :reason :not-a-sequence
   :message "the value is not a sequence"})
```

### Comments

- Inline comments: 2 spaces before `#`, stay on the same line.
- Block comments: own line, indented with surrounding code.
- A trailing comment ends its line, and that line break does not add a blank
  line before the next form.
