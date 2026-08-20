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

# The peek beside the map (docs/impl/jit.md): the map names the function a
# sampled frame belongs to, and the peek shows the instruction words at the
# frame's address. An address outside every registered block — including one
# from a photograph whose module is gone — answers nil rather than faulting.
(let [m (vm/query "jit/map" nil)]
  (when (and (vm/query "jit?" registry-probe) (> (length m) 0))
    (let* [line (get (string/split m "\n") 0)
           addr (get (string/split line " ") 0)
           words (vm/query "jit/peek" addr)]
      (assert (string? words) "jit/peek: a registered entry answers words")
      (assert (string/contains? words "0x") "jit/peek: words render as hex")))
  (assert (nil? (vm/query "jit/peek" "0x10"))
          "jit/peek: an address below every entry answers nil")
  (assert (nil? (vm/query "jit/peek" "junk"))
          "jit/peek: a malformed address answers nil"))

(println "ok: jit code-address registry answers")
