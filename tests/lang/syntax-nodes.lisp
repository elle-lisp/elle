(elle/epoch 19)
# audited: 2026-11-24
# A transformer reads a node's kind, children and span, rebuilds a node over new children, and fails the compile at a node.
# docs/macros/syntax.md

# ── syntax/kind ──────────────────────────────────────────────────────

# An atom argument reaches a transformer as a plain value, so the atoms are
# read as the children of an array.
(defmacro child-kinds (form)
  (list 'quote (map syntax/kind (syntax/children form))))

(assert (= (child-kinds [nil true 1 1.5 "s" @"s" :k sym])
           '(:nil :boolean :integer :float :string :@string :keyword :symbol))
        "an atom's kind is the type its value has")
(assert (= (child-kinds [(f) () [1] @[1] {:a 1} @{:a 1} |1| @|1| b[1] @b[1]])
           '(:list :list :array :@array :struct :@struct :set :@set :bytes
                   :@bytes)) "a compound's kind is the type its value has")
(assert (= (child-kinds ['q `q ,q ,;q ;q])
           '(:quote :quasiquote :unquote :unquote-splicing :splice))
        "each quoting form has a kind of its own")

(defmacro kind-of (form)
  (syntax/kind form))
(assert (= (kind-of {:a 1}) :struct) "a whole argument has a kind")
(assert (= (syntax/kind (syntax/<-datum nil |1 2|)) :set)
        "a node syntax/<-datum builds has a kind")

# ── syntax/children ──────────────────────────────────────────────────

(defmacro child-count (form)
  (length (syntax/children form)))
(assert (= (child-count (f 1 2)) 3) "a list's children are its elements")
(assert (= (child-count x) 0) "an atom has no children")
(assert (= (child-count 'q) 1) "a quote's one child is the form it quotes")
(assert (= (child-count {:a 1 :b 2}) 4)
        "a struct's children are its keys and values")

(defmacro struct-children (form)
  (list 'quote (map syntax/->datum (syntax/children form))))
(assert (= (struct-children {:b 2 :a 1}) '(:b 2 :a 1))
        "a struct's children keep the order the source writes")

(defmacro children-type (form)
  (type-of (syntax/children form)))
(assert (= (children-type (f 1)) :array) "the children come as an array")

(defmacro all-syntax? (form)
  (all? (fn [c] (= (type-of c) :syntax)) (syntax/children form)))
(assert (all-syntax? [1 "s" :k nil])
        "every child is a syntax object, an atom included")

# ── syntax/rebuild ───────────────────────────────────────────────────

(defmacro backwards (form)
  (syntax/rebuild form (reverse (syntax/children form))))
(assert (= (backwards [1 2 3]) [3 2 1]) "an array rebuilds as an array")
(assert (= (backwards (3 2 -)) -1) "a list rebuilds as a list")
(assert (= (type-of (backwards @[1 2])) :@array) "a mutable array stays mutable")
(assert (= (backwards {:a :b :c :d}) {:d :c :b :a})
        "a struct rebuilds as a struct")

(defmacro from-list (form)
  (syntax/rebuild form (apply list (syntax/children form))))
(assert (= (from-list [1 2]) [1 2])
        "a list holds the children as well as an array")

(defmacro same-span? (form)
  (= (syntax/span form)
     (syntax/span (syntax/rebuild form (syntax/children form)))))
(assert (same-span? (f 1)) "a rebuilt node keeps its span")

# The counter-factual: a rebuilt node without the argument's scopes would be
# the template's own, and would resolve to the top-level binding.
(def shadowed :top)
(defmacro same-atom (form)
  (syntax/rebuild form []))
(let [shadowed :local]
  (assert (= (same-atom shadowed) :local)
          "a rebuilt node keeps its scopes, and resolves at the call site"))

(def tmp 1)
(defmacro plus-tmp (form)
  (syntax/rebuild form (concat (syntax/children form) ['tmp])))
(let [tmp 100]
  (assert (= (plus-tmp (+ 10)) 11)
          "a plain child is the template's own, and resolves where the macro is defined"))

(defmacro wrap-number (form)
  (syntax/rebuild form [42]))
(assert (= (wrap-number 'q) 42) "a quote rebuilds over one plain child")

(defn rebuild-error [stx children]
  (get (get (protect (syntax/rebuild stx children)) 1) :error))
(def a-quote (first (compile/read-forms "'q" "<test>")))
(def a-symbol (first (compile/read-forms "q" "<test>")))
(assert (= (rebuild-error a-quote []) :arity-error) "a quote needs one child")
(assert (= (rebuild-error a-quote [1 2]) :arity-error)
        "a quote takes no second child")
(assert (= (rebuild-error a-symbol [1]) :arity-error) "an atom takes no child")
(assert (= (rebuild-error a-symbol {:a 1}) :type-error)
        "the children come in an array or a list")
(assert (= (rebuild-error 'q []) :type-error) "the node is a syntax object")

# ── syntax/span ──────────────────────────────────────────────────────

(def spanned
  (syntax/span (first (rest (compile/read-forms "x\n  (f y)" "<test>")))))
(assert (= [(get spanned :line) (get spanned :col)] [2 3])
        "a node's span is its line and column")
(assert (nil? (get spanned :file))
        "a source named in angle brackets has no file")
(assert (= (get (syntax/span (first (compile/read-forms "x" "dir/f.lisp")))
                :file) (path/absolute "dir/f.lisp"))
        "a node read from a file names the file as an absolute path")

(defmacro where (form)
  (let [s (syntax/span form)]
    [(get s :file) (get s :line)]))
# The trap: the two calls must stand on one line, so keep the form short
# enough that the formatter leaves it there.
(def both [(where (f)) (meta/location)])
(assert (= (first both) [(get (last both) :file) (get (last both) :line)])
        "syntax/span answers as meta/location does")

# ── syntax/error ─────────────────────────────────────────────────────

(def named-src
  "(defmacro named [form]\n  (if (syntax/symbol? form) form (syntax/error form \"wants a name\")))\n(named (f 1))")
(let [[ok? err] (protect (compile/whole-module named-src "<test>"))]
  (assert (not ok?) "a syntax error that leaves a transformer fails the compile")
  (assert (string/contains? (get err :message)
                            "<test>:3:8: macro 'named': wants a name")
          "the compile fails at the node's span, naming the macro"))

(defmacro caught (form)
  (let [[ok? err] (protect (syntax/error form "no"))]
    [ok? (get err :error) (get err :message) (syntax/symbol? (get err :syntax))]))
(assert (= (caught x) [false :syntax-error "no" true])
        "a transformer catches the error, which holds the node")

(let [[ok? err] (protect (syntax/error a-symbol "at run time"))]
  (assert (and (not ok?) (= (get err :error) :syntax-error))
          "syntax/error signals at run time as well"))
(assert (= (get (get (protect (syntax/error a-symbol :no)) 1) :error)
           :type-error) "the message is a string")

# ── a plain value is no syntax object ────────────────────────────────

(each op in [syntax/kind syntax/children syntax/span]
  (assert (= (get (get (protect (op 5)) 1) :error) :type-error)
          "a plain value has no kind, children or span"))
(assert (= (get (get (protect (syntax/error 5 "m")) 1) :error) :type-error)
        "syntax/error takes a syntax object")
