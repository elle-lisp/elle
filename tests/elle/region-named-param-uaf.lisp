(elle/epoch 12)
# Counterfactual for the named-param destructure release-before-read UAF.
#
# A `&named`/`&keys` fn compiles its parameter prologue as
# `(destructure {:name name ...} (var __named_param))` — the collected
# keyword struct is read ONCE (the inner Var) and then field-extracted by
# the Destructure node. The collected struct's region's decref_point is its
# last USE; when every destructured binding is UNUSED, that last use is the
# inner Var itself, so the lowerer frees the struct's region BEFORE the
# field extraction reads it (LIR: `decref-value-region` precedes
# `r.:name?`). Rule 4 (docs/impl/region-rules.md): a Destructure consumes its value
# after the value's last read — the value's regions extend to the
# Destructure node, exactly as Return extends a returned region.
#
# Witnessed as the lib/http2/stream.lisp import segv (the module fn takes
# `&named frame`): the freed page is recycled, the field read returns
# garbage, and downstream readers chase it — SIGSEGV/SIGABRT killing the
# corpus runner at any file importing the http2 chain
# (tests/elle/http2-session-futex.lisp). Deterministic at the pre-fix
# tree under every JIT policy; guardfree pins the free site.
#
# RED (segv) before the destructure-site decref_point extension; GREEN
# after. Pinned facets: unused &named param (the crash shape), multiple
# unused &named params, unused &keys collector, and the used-param
# control (which never crashed — its use extends the lifetime).

# the witnessed minimal shape: unused &named param, heap kwarg
(assert (= ((fn [&named frame] 42) :frame {}) 42)
        "unused &named param: collected struct must outlive the prologue")

# multiple unused named params, mixed immediate/heap kwargs
(assert (= ((fn [&named a b] 7) :a {:x 1} :b [1 2]) 7)
        "two unused &named params survive the prologue")

# unused &keys collector binding
(assert (= ((fn [&keys opts] 9) :k 1) 9)
        "unused &keys collector survives the prologue")

# control: used named param (lifetime extended by the use)
(let [r ((fn [&named frame] frame) :frame {:v 5})]
  (assert (= (get r :v) 5) "used &named param reads through"))

# the module shape: defn body + exports struct, called like an import
(def mod
  ((fn [&named frame]
     (defn f [x]
       x)
     {:f f}) :frame {}))
(assert (= ((get mod :f) 3) 3) "module-shaped &named application works")

(println "region-named-param-uaf: ok")
