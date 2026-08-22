(elle/epoch 12)
# ── Filesystem capability (:fs) ────────────────────────────────────────
#
# `:io` is the dispatch bit for requests that reach the I/O scheduler.
# The filesystem primitives are synchronous std::fs calls that never
# reach it, so `:deny |:io|` never stopped them. `:fs` is the bit that
# means "resolves a filesystem path".
#
# The trap this file guards: before `:fs` existed, the only bit that
# reached the disk was `:error`, which ~66% of primitives declare —
# including `type-of` and `length`, which the compiler emits. Denying
# `:error` to stop a write also traps the VM's own calls, and a mediator
# answering those corrupts the child silently. The compute-loop test
# below is what says `:fs` is worth having over `:error`.
#
# Arithmetic compiles to specialized bytecode that bypasses primitive
# dispatch, so enforcement tests use `length` / `type-of` (see caps.lisp).

(def root (file/mktempdir))

# ── The denial happens, and the write does not ────────────────────────

# Counterfactual: without the :fs bit this fiber runs to completion and
# the file appears on disk.
(let [victim (path/join root "ESCAPED")
      f (fiber/new (fn [] (file/write victim "x")) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "file/write under :deny |:fs| suspends")
  (assert (not (path/exists? victim)) "the denied write never reached the disk"))

(let [secret (path/join root "SECRET")]
  (file/write secret "classified")
  (let [f (fiber/new (fn [] (file/read secret)) |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "file/read under :deny |:fs| suspends")
    (assert (not (= (fiber/value f) "classified"))
            "the denied read never returned the contents")))

# ── The payload lets a parent decide by path ──────────────────────────

(let [victim (path/join root "BY-PATH")
      f (fiber/new (fn [] (file/write victim "x")) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "payload is a capability denial")
    (assert ((get v :denied) :fs) "payload names :fs as the denied bit")
    (assert (= "file/write" (get v :primitive)) "payload names the primitive")
    (assert (= victim (get (get v :args) 0))
            "payload carries the path the parent decides on")))

# The parent performs the call and resumes; the child reads the return
# value and runs on to its own result.
(let [allowed (path/join root "MEDIATED")
      f (fiber/new (fn []
                     (do
                       (file/write allowed "mediated")
                       :done)) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (let [v (fiber/value f)
        result (apply (get v :func) (get v :args))]
    (assert (= :done (fiber/resume f result))
            "the child continues to its own result")
    (assert (= (fiber/status f) :dead) "the mediated child ran to completion")
    (assert (= "mediated" (file/read allowed)) "the parent's write landed")))

# ── :fs is narrow where :error is not ─────────────────────────────────

# The regression guard for the coarseness that makes :error unusable as a
# sandbox: a loop that only computes returns the same value denied or not.
(defn compute []
  (let [@acc 0
        @i 0]
    (while (< i 200)
      (assign acc (+ acc (length "abc")))
      (assign i (+ i 1)))
    (list acc (type-of acc))))

(let [open (fiber/new compute |:fs :error|)
      denied (fiber/new compute |:fs :error| :deny |:fs|)]
  (assert (= (fiber/resume open) (fiber/resume denied))
          ":deny |:fs| does not trap length, type-of, or arithmetic")
  (assert (= (fiber/status denied) :dead)
          "a compute-only fiber under :deny |:fs| never traps"))

# ── Pure path operations stay open; path syscalls do not ──────────────

(let [f (fiber/new (fn [] (path/normalize (path/join (path/parent "/a/b") "c")))
                   |:fs :error| :deny |:fs|)]
  (assert (= "/a/c" (fiber/resume f)) "pure path string operations stay allowed")
  (assert (= (fiber/status f) :dead) "no trap for lexical path work"))

(defn denied-primitive-name [body]
  (let [f (fiber/new body |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "a filesystem syscall is denied")
    (get (fiber/value f) :primitive)))

(assert (= "path/exists?" (denied-primitive-name (fn [] (path/exists? root))))
        "path/exists? reaches the filesystem and is denied")
(assert (= "path/cwd" (denied-primitive-name (fn [] (path/cwd))))
        "path/cwd reaches the filesystem and is denied")
(assert (= "path/canonicalize"
           (denied-primitive-name (fn [] (path/canonicalize root))))
        "path/canonicalize reaches the filesystem and is denied")

# ── The port route to a path is closed too ────────────────────────────

# port/open resolves a path and reads its bytes. Without :fs on it, a
# fiber denied :fs alone would open a port on the path instead.
(let [secret (path/join root "SECRET")
      f (fiber/new (fn [] (port/open secret :read)) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "port/open under :deny |:fs| suspends")
  (assert (= "port/open" (get (fiber/value f) :primitive))
          "the denial names port/open"))

# ── The denial cannot be escaped from inside ──────────────────────────

(let [victim (path/join root "VIA-EVAL")
      f (fiber/new (fn []
                     (eval (read (string/join ["(file/write \"" victim
                                 "\" \"x\")"] "")))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (not (path/exists? victim)) "code the child evals inherits the denial"))

(let [victim (path/join root "VIA-CHILD")
      f (fiber/new (fn []
                     (let [inner (fiber/new (fn [] (file/write victim "x"))
                           |:fs :error|)]
                       (fiber/resume inner))) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (not (path/exists? victim))
          "a child fiber with an unrestricted mask cannot re-grant :fs"))

# ── Introspection, and independence from :io ──────────────────────────

(assert ((fiber/caps) :fs) "the root fiber holds :fs")

(let [f (fiber/new (fn [] 42) |:error| :deny |:fs|)]
  (assert (not ((fiber/caps f) :fs)) "fiber/caps omits :fs when denied")
  (assert ((fiber/caps f) :io) ":deny |:fs| leaves :io held"))

(let [f (fiber/new (fn [] 42) |:error| :deny |:io|)]
  (assert ((fiber/caps f) :fs) ":deny |:io| leaves :fs held")
  (assert (not ((fiber/caps f) :io)) "fiber/caps omits :io when denied"))

# ── The gate holds on a hot, already-compiled call site ───────────────

# The trap: running this file under `--jit=eager` does NOT make this claim.
# Eager compiles everything up front, so it never produces the shape that
# matters — a function the adaptive tier compiled because it got hot, then
# called from inside a denied fiber. Warm the function OUTSIDE any denial
# so it is compiled by the time the denied fiber reaches it.
(defn hot-writer [p]
  (file/write p "x"))

(let [warm (path/join root "WARM")]
  (each i (range 0 500)
    (hot-writer warm))
  (assert (path/exists? warm) "the function is warm and its writes land"))

(let [victim (path/join root "HOT-DENIED")
      f (fiber/new (fn [] (hot-writer victim)) |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "a compiled call site denies exactly as a cold one does")
  (assert (= "file/write" (get (fiber/value f) :primitive))
          "and names the same primitive")
  (assert (not (path/exists? victim)) "the hot path wrote nothing"))

(file/delete-dir-all root)
(println "caps-fs: OK")
