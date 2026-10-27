# elle fmt

<!-- audited: 2026-10-27 -->

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
([epochs.md](epochs.md)). A file with no `(elle/epoch N)` form gains a
declaration of the current epoch as its first line, and a file tagged
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

Over several files, `--check` exits 1 when any one of them needs formatting,
cannot be read, or has an error. A warning in a later file does not clear an
earlier file's error.

The check runs on files, not on stdin. It counts code delimiters only, and the
reader's lexer finds them. A `(`, `[`, or `{` inside a string literal or a
comment is text, not nesting, so the check skips it. The formatter places that
text itself and must not then report its own output.

A string literal can span lines, and the check skips its text on every line.
Escapes count: the `"` in `"a \" b"` does not close the string.

## Rule set

Every example below is the output of `elle fmt --no-epoch`, and the build runs
the examples as one program. A unit test formats each example, and fails when
the formatter would move one ([examples.rs](../src/formatter/core/tests/examples.rs)).

### General principles

- **Idempotent.** `format(format(x)) == format(x)`, always. The first pass
  gives the final layout, whatever comments and blank lines the input holds.
- **2-space indent.** Every nesting level adds 2 spaces.
- **Right of the open delimiter.** Every line inside a form starts to the
  right of the form's open delimiter, so no line reads as a sibling of the
  form it continues. The text of a multi-line string is the one exception.
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

`cond`: always breaks. Each test and body pair takes a line. A trivial body
stays beside its test when it fits there, and any other body breaks, +2 from
the test. A trailing unpaired element is the default and stands alone.

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
(defn move-note [old new]
  (cond
    (or (= old new) (= old "") (= new ""))
      (string "`" old "` onto `" new "` is not a move, so nothing changes")
    (string old " becomes " new)))
(assert (= (sign 0) :zero))
(assert (= (sign-noted -2) :negative))
(assert (= (move-note "a" "b") "a becomes b"))
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

### Header and body

`when`, `unless` and the prelude's body macros take a header and then a body.
The header stays beside the head. A single trivial body goes inline if the
whole form fits, and any other body breaks, +2. A bindings vector of name and
value pairs in the header takes the `let` layout.

| Macro | Header |
|-------|--------|
| `when`, `unless` | the test |
| `when-let`, `when-ok` | a bindings vector |
| `with-temp-dir` | the binding |
| `with-allocator` | the allocator |
| `ffi/with-stack` | the vector of stack bindings, as a collection |
| `repeat` | the count |
| `gate!` | the test and the reason |
| `with` | the binding, the constructor and the destructor |

`if-let` takes a bindings vector as its header, and lays out its two branches
as `if` does.

```lisp
(when (> answer 0)
  (push seen answer)
  answer)
(when-let [n (get {:a 1} :a)
           m (get {:b 2} :b)]
  (push seen n)
  (+ n m))
(repeat 2 (push seen :again))
(gate! (> answer 0) "needs a positive answer"
  (push seen :gated)
  :open)
(assert (= (if-let [n (get {:a 1} :a)] n 0) 1))
```

### Loops and control

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

When the first argument would start past half the line width (column 40), the
call has no room to line up its arguments after its name. They indent two
columns from the call's open paren instead. The first argument stays beside the
head if it fits on the line, and otherwise starts the next line.

```lisp
(defn call-with-zero [f]
  (f 0))
(let [message-for-the-visitor-at-the-door (string "good evening, " "visitor"
                                            ", and welcome")
      the-number-after-one-careful-step (call-with-zero
                                          (fn [n]
                                            (push seen n)
                                            (+ n 1)))]
  (assert (= message-for-the-visitor-at-the-door
             "good evening, visitor, and welcome"))
  (assert (= the-number-after-one-careful-step 1)))
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
(assert (= evens [2 4]))
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

- Inline comments: 2 spaces before `#`, stay on the same line. This holds
  for the last form of a file too.
- Block comments: own line, indented with surrounding code. Inside the
  header of a form, that is +2 from the open paren. Inside the arguments of a
  call, it is the column the arguments line up at.
- A block comment that explains a form starts on its own line above that form,
  never at the end of the line before it. This holds wherever the form sits:
  as a call's first argument, as the test of `when`, or as a value beside its
  name.
- The element after an inline comment starts the next line. In a call it
  aligns under the argument before it. In the header of a form it indents +2
  from the open paren.
- In `cond`, `match` and `case`, a body breaks, +2 from its test, when a block
  comment sits above the body or an inline comment follows the test. A `let`
  value breaks below its name in the same cases, a struct value below its key,
  and a keyword argument's value below its keyword.
- A block comment after the last element of a form stays on its own line, and
  the closing delimiter goes on the line below it.
- A trailing comment ends its line, and that line break does not add a blank
  line before the next form.
- A comment line directly below a trailing comment reads as the continuation
  of that comment. When it starts a new subject, put a blank line above it.
  The build fails on the joined shape
  ([joined_comments.rs](../tests/integration/joined_comments.rs)).

```lisp
(def queue @[])
(push queue :first)  # the job that runs first

# The second job waits for the first.
(push queue :second)
(assert (= (length queue) 2))
(defn pick [x]
  (cond
    (= x 1) :one
    true
      # Every other value lands here.
      :other))
(assert (= (pick 2) :other))
(defn parity [n]
  (cond
    (even? n)  # zero counts as even
      :even
    :odd))
(assert (= (parity 0) :even))
(def total
  (+ 1  # the base
     2))
(assert (= total 3))
(let [x 1
      y
        # The comment stays above the value.
        2]
  (assert (= (+ x y) 3)))
(def steps
  [:fetch
   :parse
   # A step added here runs last.
   ])
(assert (= (length steps) 2))
```

### Blank lines

- A run of blank lines collapses to at most two.
- A blank line survives where the layout starts the element after it on a new
  line, as it does between top-level forms, between body forms, and between
  comments.
- A blank line never decides the layout. Where the element after it shares a
  line with the element before, the blank line is dropped. A `when` with one
  trivial body below a blank line still goes inline when it fits.
- Blank lines before a closing delimiter are dropped.
