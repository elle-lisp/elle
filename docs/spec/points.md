# The points of the region IR

<!-- audited: 2026-11-24 -->

Where each step of the region IR lies, which step follows it, what it reads, and what a jump hands to a block's parameters.

[The facts](facts.md) says what each step gives besides, and
[datalog](datalog.md) defines `next`, `use`, `pass` and `alloc`.

## Points

A point is an instruction or a terminator, numbered in view order: the blocks
in the order of `:blocks`, and each block's instructions before its
terminator. `next` follows instruction order and each terminator's targets.
Two kinds of point have no node of their own:

| Point | What it is |
|-------|------------|
| `[:exit p]` | the exit of the Return or the TailCall at `p` |
| `[:edge p l]` | the edge from the Branch at `p` to the block `l`, where `l` takes parameters |

Each Return has an exit of its own, because a shared exit would keep a value
returned on one path live behind every other return. A TailCall returns at its
own point, because a closure callee replaces the frame there. Its fall-through
runs only for a native callee.

## Uses

Every register a step reads gives `use(v, p)` for the value it holds, a callee
and a branch's condition included. A Release gives no use, and an argument a
jump hands to a block parameter gives `pass` and no use.

## Terminators and block parameters

A block parameter is a register its block defines. A jump hands it a value,
and so the step that hands it is a `pass`, as the Recur rule of
[the ownership rules](ownership.md) reads one.

| Kind | Facts |
|------|-------|
| TailCall | `next(p, [:exit p])`, beside the `next` to the instruction after it |
| Return | `next(p, [:exit p])` |
| Jump | `next` to its target, and `pass(y, x, p)` for each argument `y` and the target's parameter `x` at its position |
| Branch | `next` to the target that takes no parameter; `next(p, [:edge p l])` to a target `l` that takes some |
| an edge point `[:edge p l]` | `next` to the first point of `l`, and `pass(y, x, [:edge p l])` for each argument `y` of the Branch to `l` |
| Emit | `next` to its resume block, and `alloc(x, p)` for that block's one parameter `x` |
| Unreachable | no `next` |

A Branch hands its arguments on the edge, because only one edge runs. A pass
at the Branch's own point would move each argument on both edges, and release
the other target's parameter as unused. An Emit's resume value comes from the
fiber that resumes this one, and a resume moves its value, so this fiber owns
it from the Emit on.

## The module

`points` answers a function's nodes in view order, each as `{:node n :term?
b}`, beside its `:next`, `:use`, `:pass` and `:alloc` facts. `function` and
`make-block` build a hand-built unit, and `same?` compares two sets of facts.
The helper is not named `block`, because a call to that name runs the special
form:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))

  # The fields that hold a register, for each kind that reads one other than :src.
  (def fields
    {:list [:head :tail] :make-array-mut [:elements] :first [:pair] :rest [:pair]
     :bin-op [:lhs :rhs] :compare [:lhs :rhs] :identical [:lhs :rhs] :make-closure [:captures]
     :call [:func :args] :suspending-call [:func :args] :tail-call [:func :args]
     :call-array-mut [:func :args] :tail-call-array-mut [:func :args] :make-capture-cell [:value]
     :load-capture-cell [:cell] :store-capture-cell [:cell :value] :eval [:expr :env]
     :array-mut-extend [:array :source] :array-mut-push [:array :value]
     :intr-push [:array :value] :intr-string-push [:string :value] :intr-bytes-push [:bytes :value]
     :get [:obj :key] :del [:obj :key] :has [:obj :key] :put [:obj :key :val]
     :push-param-frame [:pairs] :adopt-region [:parent :child] :adopt-cell-region [:parent :child]
     :adopt-into-activation [:child] :free-region-group [:members] :return [:value]
     :branch [:cond] :emit [:value]})

  (defn reads [node]
    "The registers NODE reads, beside a Release's value and a jump's arguments."
    (def kind (get node :kind))
    (def held (if (has? fields kind) (get fields kind) (if (has? node :src) [:src] [])))
    (mapcat (fn [f] (def r (get node f)) (if (array? r) r [r])) held))

  (defn points [fun]
    "FUN's nodes in view order, and the next, use, pass and alloc facts its points give."
    (def nodes @[])
    (def starts @{})
    (def params @{})
    (each b in (get fun :blocks)
      (put starts (get b :label) (length nodes))
      (put params (get b :label) (get b :params []))
      (each instr in (get b :instrs) (push nodes {:node instr :term? false}))
      (push nodes {:node (get b :term) :term? true}))
    (def next @[])
    (def use @[])
    (def pass @[])
    (def alloc @[])
    (defn hand [from l args]
      (def xs (get params l))
      (each i in (range (length xs)) (push pass [(get args i) (get xs i) from])))
    (defn enter [p l args]
      (if (empty? (get params l))
        (push next [p (get starts l)])
        (let [e [:edge p l]]
          (push next [p e])
          (push next [e (get starts l)])
          (hand e l args))))
    (each p in (range (length nodes))
      (def node (get (get nodes p) :node))
      (each r in (reads node) (push use [r p]))
      (match (get node :kind)
        :return (push next [p [:exit p]])
        :jump (let [l (get node :target)]
                (push next [p (get starts l)])
                (hand p l (get node :args [])))
        :branch (begin (enter p (get node :then-label) (get node :then-args []))
                       (enter p (get node :else-label) (get node :else-args [])))
        :emit (let [l (get node :resume-label)]
                (push next [p (get starts l)])
                (each x in (get params l) (push alloc [x p])))
        :unreachable nil
        :tail-call (begin (push next [p (+ p 1)]) (push next [p [:exit p]]))
        _ (push next [p (+ p 1)])))
    {:nodes (freeze nodes) :next (freeze next) :use (freeze use) :pass (freeze pass)
     :alloc (freeze alloc)})

  (defn same? [want got]
    "Whether the facts GOT hold the keys of WANT, each key's tuples in any order."
    (and (= (sort (keys want)) (sort (keys got)))
         (all? (fn [k] (= (sort (get want k)) (sort (get got k)))) (keys want))))

  (defn function [blocks &named id params captures]
    "A function of a hand-built unit, its entry the block labeled 0."
    {:id id :name nil :params (or params 0) :captures (or captures []) :slots [] :entry 0
     :blocks blocks})

  (defn make-block [label instrs term &named params]
    "A block of a hand-built unit, with the registers of its PARAMS."
    {:label label :params (or params []) :instrs instrs :term term})

  (defn flows [fun]
    "A check that FUN's points give the facts a fence holds."
    (fn [want]
      (def got (points fun))
      (same? want {:next (get got :next) :pass (get got :pass) :alloc (get got :alloc)})))

  (def edge (fence 'edge
    :doc "A Branch hands its arguments on the edge to the block that takes them, and a Jump at its own point."
    :facts {:next [[0 1] [1 2] [2 3] [2 [:edge 2 2]] [[:edge 2 2] 5] [3 4] [4 5] [5 [:exit 5]]]
            :pass [[1 5 [:edge 2 2]] [3 5 4]] :alloc []}
    :check (flows (function [(make-block 0 [{:kind :param :dst 0 :index 0}
                                       {:kind :make-array-mut :dst 1 :elements []}]
                                    {:kind :branch :cond 0 :then-label 1 :else-label 2 :else-args [1]})
                             (make-block 1 [{:kind :make-array-mut :dst 3 :elements []}]
                                    {:kind :jump :target 2 :args [3]})
                             (make-block 2 [] {:kind :return :value 5} :params [5])] :params 1))))

  (def exits (fence 'exits
    :doc "A TailCall has an exit beside its fall-through, an Emit makes its resume parameter, and an Unreachable leads nowhere."
    :facts {:next [[0 1] [1 2] [1 [:exit 1]] [2 3]] :pass [] :alloc [[2 2]]}
    :check (flows (function [(make-block 0 [{:kind :const :dst 0 :value 1}
                                       {:kind :tail-call :dst 1 :func 0 :args []}]
                                    {:kind :emit :signal |:yield| :value 1 :resume-label 1})
                             (make-block 1 [] {:kind :unreachable} :params [2])]))))

  (def reads (fence 'reads
    :doc "A step uses each register it reads, a callee included, and a Release and a jump's arguments use none."
    :facts {:use [[0 2] [2 2] [1 3] [2 3] [5 6]]}
    :check (fn [want]
             (same? want {:use (get (points (function [(make-block 0 [{:kind :param :dst 0 :index 0}
                                                                     {:kind :const :dst 2 :value nil}
                                                                     {:kind :call :dst 1 :func 0 :args [2]}
                                                                     {:kind :store-capture-cell :cell 1 :value 2}
                                                                     {:kind :release :value 1}]
                                                                    {:kind :jump :target 1 :args [2]})
                                                       (make-block 1 [] {:kind :return :value 5} :params [5])]))
                                  :use)}))))

  {:points points :same? same? :function function :make-block make-block :fences [edge exits reads]})
```
