(elle/epoch 13)
# audited: 2026-09-29
# The page-claims tool: how it ranks a callgrind file, and how it reads its
# arguments.
# docs/impl/region/colocation.md
#
# The callgrind file is written out below rather than produced by valgrind, so
# this runs wherever the corpus runs. Its shapes are the ones a real
# `--separate-callers12=*add_page*` run writes.
#
# The trap: callgrind names a function once, as `cfn=(ID) NAME`, and every later
# mention is the bare `cfn=(ID)`. A `fn=(ID) NAME` line names an ID too. A reader
# that counts only the lines carrying a name undercounts every context after its
# first call, and the ranking below would read 2 where it must read 5.

(def {:rank rank} ((import-file "tools/pageclaims/rank.lisp")))
(def {:options options :commands commands}
  ((import-file "tools/pageclaims/options.lisp")))

(defn reader [lines]
  "A read-line thunk over LINES: the next line on each call, then nil."
  (var i 0)
  (fn []
    (when (< i (length lines))
      (def line (get lines i))
      (assign i (+ i 1))
      line)))

(defn context [& frames]
  "The name callgrind gives one caller context of add_page."
  (string/join (concat ["<elle::value::fiberheap::regionpool::RegionPool>::add_page"
                        "<elle::value::fiberheap::regionpool::RegionPool>::alloc_data"]
                       (->array frames)) "'"))

(def vm "<elle::vm::core::VM>::")

(def fixture
  ["# callgrind format" "version: 1" "events: Ir"
   (string "fn=(1) " vm "run_dispatch") "0 10"
   # A closure payload: named once, then called again by ID alone.
   (string "cfn=(2) "
           (context "<elle::value::fiberheap::FiberHeap>::template_payload"
                    "elle::vm::closure::materialize_closure_in_region"
                    (string vm "execute_bytecode_inner_impl")
                    (string vm "run_dispatch") (string vm "execute_code")))
   "calls=2 353" "* 330" "cfn=(2)" "calls=3 353" "* 330"
   # A native's array, with object and file lines between the name and its calls.
   (string "cfn=(3) "
           (context "elle::value::build::array"
                    "elle::primitives::list::prim_to_array"
                    (string vm "dispatch_native_call") (string vm "call_inner")))
   "cob=(1) elle" "cfi=(1) src/primitives/list.rs" "calls=5 400" "* 12"
   # The same native through two builders: once past the plumbing, the same path.
   (string "cfn=(6) "
           (context "elle::value::build::struct_from_sorted"
                    "elle::value::build::struct_from"
                    "elle::primitives::list::prim_to_array"
                    (string vm "dispatch_native_call") (string vm "call_inner")))
   "calls=1 400" "* 12"
   # A call to something other than add_page counts nothing.
   "cfn=(4) <elle::value::fiberheap::regionpool::RegionPool>::alloc_data'elle::main"
   "calls=100 1" "* 1"
   # An environment value: its context is named by a `fn=` line, and the call
   # names it by ID alone.
   (string "fn=(5) "
           (context "<elle::value::fiberheap::FiberHeap>::alloc_in_region"
                    (string vm "populate_env") (string vm "build_closure_env")
                    (string vm "call_inner") (string vm "handle_call"))) "0 1"
   (string "fn=(1) " vm "run_dispatch") "cfn=(5)" "calls=4 1" "* 1"
   # A tie with the environment value, which the path's text breaks.
   (string "cfn=(7) "
           (context "<elle::syntax::node::Syntax>::copy_into"
                    "<elle::syntax::expand::Expander>::expand_macro_call"
                    "<elle::syntax::expand::Expander>::expand")) "calls=4 9"
   "* 1"])

(def closure-path
  "vm::closure::materialize_closure_in_region <- <vm::core::VM>::execute_bytecode_inner_impl <- <vm::core::VM>::run_dispatch")
(def native-path
  "primitives::list::prim_to_array <- <vm::core::VM>::dispatch_native_call <- <vm::core::VM>::call_inner")
(def env-path
  "<vm::core::VM>::populate_env <- <vm::core::VM>::build_closure_env <- <vm::core::VM>::call_inner")
(def syntax-path
  "<syntax::node::Syntax>::copy_into <- <syntax::expand::Expander>::expand_macro_call <- <syntax::expand::Expander>::expand")

# ── ranking ─────────────────────────────────────────────────────────────────

(def ranked (rank (reader fixture) 3))
(assert (= (get ranked :total) 19)
        "the total counts every call to add_page, and no call to anything else")
(assert (= (get ranked :paths)
           [[6 native-path] [5 closure-path] [4 syntax-path] [4 env-path]])
        "the paths rank by claims, most first, and a tie ranks by the path's text")

(def shallow (rank (reader fixture) 1))
(assert (= (get shallow :total) 19) "the depth does not change the total")
(assert (= (get shallow :paths)
           [[6 "primitives::list::prim_to_array"]
            [5 "vm::closure::materialize_closure_in_region"]
            [4 "<syntax::node::Syntax>::copy_into"]
            [4 "<vm::core::VM>::populate_env"]])
        "a depth of 1 keeps the first frame past the plumbing")

(def empty (rank (reader []) 3))
(assert (= (get empty :total) 0) "an empty file claims nothing")
(assert (empty? (get empty :paths)) "and ranks no path")

# ── arguments ───────────────────────────────────────────────────────────────

(def defaults (options []))
(assert (= (get defaults :depth) 3) "the depth defaults to 3")
(assert (= (get defaults :top) 20) "twenty paths print unless told otherwise")
(assert (= (get defaults :elle) "target/profiling/elle")
        "the profiled binary is the profiling build")
(assert (empty? (get defaults :args)) "and it runs with no arguments")

# elle hands the program the `--` that ended its own flags, so the tool drops a
# leading one. Everything after the tool's options belongs to the profiled
# elle, flags included.
(def given
  (options ["--" "--depth" "5" "--top" "7" "--elle" "x/elle" "test" "--jit=off"
            "a.lisp"]))
(assert (= (get given :depth) 5) "--depth sets the depth")
(assert (= (get given :top) 7) "--top sets how many paths print")
(assert (= (get given :elle) "x/elle") "--elle names the profiled binary")
(assert (= (get given :args) ["test" "--jit=off" "a.lisp"])
        "the rest is the profiled elle's, in order")

(assert (= (get (options ["a.lisp" "--depth" "2"]) :args)
           ["a.lisp" "--depth" "2"])
        "the options end at the first argument that is not one")
(assert (= (get (options ["a.lisp" "--depth" "2"]) :depth) 3)
        "so an option after the program is the program's")

(defn refused? [args]
  (let [[ok? _] (protect (options args))]
    (not ok?)))
(assert (refused? ["--depth" "x"]) "a depth that is not an integer is refused")
(assert (refused? ["--depth" "0"]) "a path has at least one frame")
(assert (refused? ["--top" "0"]) "at least one path prints")
(assert (refused? ["--depth"]) "an option with no value is refused")

# ── commands ────────────────────────────────────────────────────────────────
# The trap: the profiled elle shares the stdlib cache under $TMPDIR with every
# other elle, and a store by one binary removes the others' files. Profiled
# against that cache, one program read 4175 page claims on one run and 1729 on
# the next. So both commands name a cache inside the run's own directory, and
# the warm-up fills it before callgrind runs.

(def planned (commands given "/scratch"))
(assert (= (get planned :out) "/scratch/callgrind.out")
        "callgrind writes into the run's directory")
(assert (= (get planned :empty) "/scratch/empty.lisp")
        "the warm-up runs an empty program from the run's directory")
(assert (= (get planned :warm)
           ["x/elle" "--cache=/scratch/cache" "/scratch/empty.lisp"])
        "the warm-up fills the run's own cache, outside callgrind")
(assert (= (get planned :profile)
           ["valgrind" "--tool=callgrind" "--dump-instr=no"
            "--separate-callers12=*add_page*"
            "--callgrind-out-file=/scratch/callgrind.out" "x/elle"
            "--cache=/scratch/cache" "test" "--jit=off" "a.lisp"])
        "the profiled elle reads that cache, and then takes its own arguments")

(println "page-claims: ok")
