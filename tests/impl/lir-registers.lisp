(elle/epoch 18)
# audited: 2026-11-21
# compile/lir writes each register once in each function of a unit.
# src/lir/overview.md
#
# The extractor names the value a register holds by the one instruction that
# writes it. A lowering that reads a spilled value back into the register it
# came from writes that register twice, and a reader of the data form can no
# longer say which write a read sees.

(def source
  (string "(defn spill [a] (fn [] (length a)))\n"
          "(defn adopt [] (def p @[1 2]) (def k (fn [] (length p))) (length p) (k))\n"
          "(defn relay [p] (fn [] (yield p)))\n"
          "(defn drive [f] (def w @[1]) (fiber/resume f w) (yield w) nil)\n"
          "(defn pick [x] (match x [a b] a {:k v} v _ nil))\n"
          "(defn walk [xs acc] (if (empty? xs) acc (walk (rest xs) (pair (first xs) acc))))\n"))
(def unit (compile/lir source "<lir-registers>"))

(defn writes [f]
  "The register each instruction of F writes, once per write; a tail call's dst counts."
  (filter int?
          (mapcat (fn [b] (map (fn [i] (get i :dst)) (get b :instrs)))
                  (get f :blocks))))

(defn rewritten [f]
  "The registers F writes more than once."
  (def seen @||)
  (def twice @[])
  (each r in (writes f)
    (if (has? seen r) (push twice r) (add seen r)))
  twice)

(assert (> (length unit) 6) "the unit holds the entry function and each defn")
(each f in unit
  (assert (empty? (rewritten f))
          (string "no register is written twice in function " (get f :id))))
