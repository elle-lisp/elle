(elle/epoch 12)
# The JIT code-address registry (docs/impl/jit.md, "The code-address
# registry").
#
# A native thread photograph names every frame it can symbolize; a JIT frame
# shows only `??? (in <unknown binary>)`, because the code lives in an
# anonymous Cranelift mapping. The registry is the missing symbol table: each
# successful compile records its entry address and function name, and
# `(vm/query "jit/map" nil)` renders the table for the reader holding such a
# photograph. The test runner prints it beside the photograph when a form
# misses its deadline.
#
# A compiled function's registry label is its declared name when one exists,
# else its source location — and lowering names almost nothing, so the
# location, which carries this FILE's name, is what identifies the probe
# below. Under `--jit=off` nothing compiles here and the map may even be
# empty; the render is a string either way.

(defn registry-probe [x]
  (if x x 1))

(def @i 0)
(while (< i 50)
  (registry-probe i)
  (assign i (+ i 1)))

# Drain pending background compilations so a compile that was still in
# flight lands before the map is read.
(jit/rejections)
(registry-probe 0)

(let [m (vm/query "jit/map" nil)]
  (assert (string? m) "jit/map: renders as a string")
  (when (vm/query "jit?" registry-probe)
    (assert (string/contains? m "jit-registry")
            "jit/map: a compiled function's source label appears in the registry")))

(println "ok: jit code-address registry answers")
