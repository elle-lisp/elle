(elle/epoch 18)
# audited: 2026-11-24
# Where present reads and writes a region: a register, a local slot, an environment index, a cell or a static region.
# tools/glean/present.md

(fn []
  (def {:refuse refuse
        :record record
        :with-record with-record
        :lower lower
        :raise raise
        :make make} ((import "./regions")))

  # A state keeps only the registers and slots that hold a region, so two paths
  # that differ only in their immediates reach a point in one state.
  (defn holding [m k tok]
    "M with K holding TOK, or with no K where TOK is :none."
    (if (= tok :none) (del m k) (put m k tok)))

  (defn reg [ctx state r]
    "The region register R holds, refusing a register no step writes."
    (unless (has? (get ctx :written) r)
      (refuse :unwritten (string "no step writes register " r)))
    (get (get state :regs) r :none))

  (defn set-reg [state r tok]
    (put state :regs (holding (get state :regs) r tok)))

  (defn slot [state s]
    (get (get state :slots) s :none))

  (defn set-slot [state s tok]
    (put state :slots (holding (get state :slots) s tok)))

  (defn env [ctx state i]
    "The region at environment index I: a capture's, or a slot's past them."
    (def ncaps (get ctx :ncaps))
    (if (< i ncaps) [:capture i] (slot state (- i ncaps))))

  (defn named [ctx r]
    "The glean names of the value register R holds, as a set."
    (def names (get ctx :names))
    (def v (names:name r))
    (if (nil? v) || (set v)))

  (defn give [ctx state node p count]
    "STATE with the step NODE at P giving its register a region of COUNT,
  mapped by its static region where it carries one."
    (make ctx state p (get node :dst) (get node :region) count
          (named ctx (get node :dst))))

  (defn set-content [state cell tok]
    "STATE with the cell CELL holding TOK, where CELL is a cell."
    (def rec (if (= cell :none) nil (record state cell)))
    (if (and rec (has? rec :content))
      (with-record state cell (put rec :content tok))
      state))

  (defn by-static [ctx state r p how]
    "Raise or release the region the static region R maps at P, HOW :raise or
  :release; a release takes the mapping."
    (def tok (get (get state :map) r))
    (cond
      (nil? tok) state
      (= how :raise) (raise ctx state tok p)
      (let [s (lower ctx state tok p :release)]
        (put s :map (del (get s :map) r)))))

  {:reg reg
   :set-reg set-reg
   :slot slot
   :set-slot set-slot
   :env env
   :named named
   :give give
   :set-content set-content
   :by-static by-static})
