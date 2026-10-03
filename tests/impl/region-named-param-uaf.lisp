(elle/epoch 13)
# audited: 2026-09-29
# A &named or &keys prologue reads its collected struct before the struct's region is released.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, so a field read of the freed struct faults.
#
# A `&named`/`&keys` fn compiles its parameter prologue as
# `(destructure {:name name ...} (var __named_param))` — the collected
# keyword struct is read ONCE (the inner Var) and then field-extracted by
# the Destructure node. Rule 4: a Destructure consumes its value after the
# value's last read, so the value's regions extend to the Destructure node,
# exactly as Return extends a returned region.
#
# The counter-factual: with every destructured binding UNUSED, the struct's
# last use is the inner Var itself, and a decref_point there frees the
# struct's region BEFORE the field extraction reads it (LIR:
# `decref-value-region` precedes `r.:name?`). The freed page is recycled, the
# field read returns garbage, and its readers crash the process. A module
# function that takes `&named frame`, as lib/http2/stream.lisp does, is the
# shape. The used-param control keeps its struct alive through the use.

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
