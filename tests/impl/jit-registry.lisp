(elle/epoch 15)
# audited: 2026-10-20
# Each compiled function's entry address and name are in the JIT code-address registry.
# docs/impl/jit-map.md
#
# Binder names reach closure payloads (docs/functions.md), so the probe is
# labeled by its defn name below. With the JIT off (a sidecar, or a build
# without the JIT tier) nothing compiles here and the map may be empty; the
# render is a string either way.

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
    (assert (string/contains? m "registry-probe")
            "jit/map: a compiled function's name appears in the registry")))

# Each line of the peek window names its own address, and an address outside
# every registered block answers nil rather than faulting.
(let [m (vm/query "jit/map" nil)]
  (when (and (vm/query "jit?" registry-probe) (> (length m) 0))
    (let* [line (get (string/split m "\n") 0)
           addr (get (string/split line " ") 0)
           words (vm/query "jit/peek" addr)]
      (assert (string? words) "jit/peek: a registered entry answers words")
      (assert (string/contains? words "0x") "jit/peek: words render as hex")
      (assert (> (length (string/split words "\n")) 1)
              "jit/peek: the window spans several address-prefixed lines")
      (each l in (string/split words "\n")
        (assert (string/contains? l ":")
                "jit/peek: each window line names its address"))))
  (assert (nil? (vm/query "jit/peek" "0x10"))
          "jit/peek: an address below every entry answers nil")
  (assert (nil? (vm/query "jit/peek" "junk"))
          "jit/peek: a malformed address answers nil"))

(println "ok: jit code-address registry answers")
