(elle/epoch 18)
# audited: 2026-11-24
# What a call does to the regions present walks: the closure arm and the native arm, and the points each goes on to.
# tools/glean/present.md

(fn []
  (def {:content content :lower lower :exit exit} ((import "./regions")))
  (def {:reg reg :slot slot :set-slot set-slot :give give :by-static by-static}
    ((import "./slots")))

  (def calls
    |:call :suspending-call :tail-call :call-array-mut :tail-call-array-mut|)

  (defn arms [ctx state node]
    "The arms of a call: [:native] or [:closure] for a callee a ValueConst
  names, both for any other."
    (reg ctx state (get node :func))
    (def names (get ctx :names))
    (def writer (names:written (get node :func)))
    (def v (when (= (get writer :kind) :value-const) (get writer :value)))
    (cond
      (native-fn? v) [:native]
      (closure? v) [:closure]
      [:closure :native]))

  (defn hand-moves [ctx state node p]
    "STATE after a closure callee takes each move of the call NODE at P."
    (fold (fn [s [_ s-slot]]
            (set-slot (lower ctx s (slot s s-slot) p :hand) s-slot :none)) state
          (get node :moves)))

  (defn tail-closure [ctx state node p]
    "Answer the exit of a closure's TailCall NODE at P."
    (def late (set ;(map (fn [m] (get m 0)) (get node :late-mints))))
    (def args (get node :args))
    (var s state)
    (each i in (range (length args))
      (unless (contains? late i)
        (assign s (lower ctx s (content s (reg ctx s (get args i))) p :hand))))
    (when (get node :defer-callee-release)
      (assign s (lower ctx s (reg ctx s (get node :func)) p :release)))
    (assign
      s
      (fold (fn [s slot] (by-static ctx s slot p :release)) s
            (get node :deferred-release-slots)))
    (exit ctx s p))

  (defn call-step [ctx state node p]
    "The points and states after the call NODE at P, one per arm."
    (def kind (get node :kind))
    (def go (+ p 1))
    (cond
      (= kind :call-array-mut)
        [[go
          (give ctx (by-static ctx state (get node :args-region) p :release)
                node p 1)]]
      (= kind :tail-call-array-mut)
        (let [s (by-static ctx state (get node :args-region) p :release)]
          (mapcat (fn [arm]
                    (if (= arm :closure)
                      (begin
                        (exit ctx s p)
                        [])
                      [[go s]])) (arms ctx state node)))
      (= kind :tail-call)
        (mapcat (fn [arm]
                  (if (= arm :closure)
                    (begin
                      (tail-closure ctx state node p)
                      [])
                    [[go (give ctx state node p 1)]])) (arms ctx state node))
      (map (fn [arm]
             [go
              (give ctx
                    (if (= arm :closure) (hand-moves ctx state node p) state)
                    node p 1)]) (arms ctx state node))))

  {:calls calls :call-step call-step})
