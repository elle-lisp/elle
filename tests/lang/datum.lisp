(elle/epoch 19)
# audited: 2026-11-24
# read, read-all and syntax/->datum answer a literal as the value it writes, and syntax/<-datum builds a node of the same kind back.
# docs/syntax.md
# docs/macros/syntax.md

# ── read ─────────────────────────────────────────────────────────────

(assert (= (read "{:a 1}") {:a 1}) "a struct literal reads as a struct")
(assert (= (type-of (read "@{:a 1}")) :@struct)
        "a mutable struct literal reads as an @struct")
(assert (= (read "|1 2|") |1 2|) "a set literal reads as a set")
(assert (= (type-of (read "@|1 2|")) :@set)
        "a mutable set literal reads as an @set")
(assert (= (read-all "{:a 1} |2|") (list {:a 1} |2|))
        "read-all reads each literal as read does")

(assert (= (read "{:a (f x)}") {:a '(f x)})
        "a part of a literal that is code stays code")
(assert (= (read "|x (f)|") |'x '(f)|) "a set's elements are data")
(assert (= (read "[{:a 1}]") [{:a 1}]) "a nested literal reads as its value")
(assert (= (read "({:a 1})") (list {:a 1}))
        "a literal inside a list reads as its value")
(assert (= (read "(struct :a 1)") '(struct :a 1))
        "a list that calls struct stays a list")

(assert (= (read "b[1 2]") '(bytes 1 2)) "a bytes literal reads as a call")
(assert (= (read "@b[1 2]") '(@bytes 1 2))
        "a mutable bytes literal reads as a call")
(assert (= '{:a 1} '(struct :a 1)) "a quoted struct literal is a call")
(assert (= '|1| '(set 1)) "a quoted set literal is a call")

(assert (= (eval (read "{:a (+ 1 2)}")) {:a 3})
        "eval runs a read struct as the literal runs")
(assert (= (eval (read "|(+ 1 2)|")) |3|)
        "eval runs a read set as the literal runs")
(let [m (eval (read "@{:a 1}"))]
  (put m :b 2)
  (assert (= (get m :b) 2) "eval of a read @struct builds a mutable one"))

# ── what the value decides ───────────────────────────────────────────

(assert (= (read "{:a 1 :a 2}") {:a 2})
        "a repeated key keeps its last value, as at run time")
(assert (= (read "|x x|") |'x|) "a repeated element is one element")
(assert (= (length (read "@|(f) (f)|")) 1)
        "a repeated element of a mutable set is one element")
(assert (= (read "{:b 1 :a 2}") {:a 2 :b 1})
        "the keys come in the value's order")

(defn read-error [text]
  (get (get (protect (read text)) 1) :error))
(assert (= (read-error "{1.5 :x}") :type-error) "a float key fits no struct")
(assert (= (read-error "{@[1] :x}") :type-error) "a mutable key fits no struct")
(assert (= (read-error "@{1.5 :x}") :type-error)
        "a float key fits no mutable struct either")
(assert (= (read "{(f x) 1}") {'(f x) 1}) "a key that is code is data")

# ── syntax/->datum ───────────────────────────────────────────────────

(defmacro datum-type (x)
  (type-of (syntax/->datum x)))
(assert (= (datum-type {:a b}) :struct) "a struct argument's datum is a struct")
(assert (= (datum-type @{:a b}) :@struct)
        "a mutable struct argument's datum is an @struct")
(assert (= (datum-type |a|) :set) "a set argument's datum is a set")
(assert (= (datum-type (struct :a b)) :list)
        "a list that calls struct stays a list")
(assert (= (datum-type b[1]) :list) "a bytes argument's datum is a call")

(defn datum-of [text]
  (syntax/->datum (first (compile/read-forms text "<test>"))))
(assert (= (datum-of "{:a (f x)}") (read "{:a (f x)}"))
        "syntax/->datum answers what read answers")
(assert (= (datum-of "'{:a 1}") (list 'quote {:a 1}))
        "a quote's datum holds the literal's value")
(assert (= (datum-of "{:a 1 :a 2}") {:a 2})
        "a repeated key keeps its last value")
(assert (= (get (get (protect (datum-of "{1.5 :x}")) 1) :error) :type-error)
        "a key no struct holds signals")

# ── syntax/<-datum back ──────────────────────────────────────────────

(defn round-trip [text]
  (syntax/<-datum nil (datum-of text)))
(assert (= (syntax/kind (round-trip "{:a b}")) :struct)
        "a struct datum builds a struct node")
(assert (= (syntax/kind (round-trip "@|a|")) :@set)
        "an @set datum builds an @set node")
(assert (= (syntax/kind (round-trip "(struct :a b)")) :list)
        "a list datum builds a list node")
(assert (= (syntax/kind (round-trip "{(f x) 1}")) :struct)
        "a key that is code builds back")
(assert (= (syntax/->datum (round-trip "{(f x) |y|}")) {'(f x) |'y|})
        "the node holds what the datum held")
