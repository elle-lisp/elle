(elle/epoch 18)
# audited: 2026-11-24
# The present releases: where each function's count instructions end its hold on each value's region.
# tools/glean/present.md

(fn []
  (def kinds ((import "./kinds")))
  (def {:refuse refuse} ((import "./regions")))
  (def {:view view} ((import "./points")))
  (def {:naming naming} ((import "./names")))
  (def {:step step} ((import "./steps")))
  (def {:live-after live-after :prune prune} ((import "./alive")))

  (defn merged [seen]
    "The static regions two or more steps of SEEN carry, which the lowerer merged."
    (def carried @{})
    (each entry in (get seen :nodes)
      (let [r (get (get entry :node) :region)]
        (unless (nil? r)
          (put carried r (+ 1 (get carried r 0))))))
    (set ;(map (fn [[r n]] r) (filter (fn [[r n]] (> n 1)) (pairs carried)))))

  (defn written [seen]
    "The registers some step of SEEN writes."
    (set ;(filter (fn [r] (not (nil? r)))
                  (map (fn [entry] (get (get entry :node) :dst))
                       (get seen :nodes)))))

  (defn count-bound [seen]
    "One plus the number of steps of SEEN that raise a count."
    (+ 1
       (length (filter (fn [entry]
                         (has? |:incref-region :incref-value-region|
                               (get (get entry :node) :kind))) (get seen :nodes)))))

  (defn entry [fun names]
    "The state at FUN's entry: each parameter's region in its slot, counted
  once, and each capture's region, counted never."
    (def regions @{})
    (def slots @{})
    (each i in (range (get fun :params))
      (put slots i [:param i])
      (put regions [:param i]
           {:count 1
            :mode :counted
            :values (set (names:slot-name i))
            :carries ||}))
    (each i in (range (length (get fun :captures)))
      (put regions [:capture i]
           {:count 0
            :mode :counted
            :values (set (names:capture-name i))
            :carries ||}))
    {:regs {} :slots (freeze slots) :map {} :regions (freeze regions) :dues ||})

  (defn walk [ctx start live]
    "Walk every path from START at point 0, each state once per point, each
  state after a point pruned to what LIVE says a later step reads."
    (def visited @{})
    (def work @[[0 start]])
    (while (nonempty? work)
      (let [[p state] (pop work)
            seen-at (get visited p)]
        (unless (and seen-at (contains? seen-at state))
          (if seen-at (add seen-at state) (put visited p (add @|| state)))
          (each [q s] in (step ctx state p)
            (push work [q (prune s (get live p))]))))))

  (defn function-answer [fun]
    "The struct present answers for FUN, refusing a kind glean does not know."
    (def seen (view fun))
    (each entry in (get seen :nodes)
      (let [kind (get (get entry :node) :kind)]
        (unless (kinds:known? kind)
          (refuse :unknown-kind (string "no rule knows the kind " kind)))))
    (def names (naming fun seen))
    (def out {:released @|| :handed @|| :held @|| :over @||})
    (walk {:view seen
           :names names
           :ncaps (length (get fun :captures))
           :merged (merged seen)
           :written (written seen)
           :bound (count-bound seen)
           :out out} (entry fun names) (live-after fun seen))
    {:id (get fun :id)
     :name (get fun :name)
     :released (->array (get out :released))
     :handed (->array (get out :handed))
     :held (->array (get out :held))
     :over (->array (get out :over))})

  (defn present [unit]
    "The present releases of each function of UNIT."
    {:functions (->array (map function-answer unit))})

  {:present present})
