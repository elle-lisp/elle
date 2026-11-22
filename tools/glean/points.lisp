(elle/epoch 18)
# audited: 2026-11-22
# Glean's points: each instruction and terminator of a function in view order, and the next facts that link them.
# tools/glean/overview.md

(fn []
  (defn view [fun]
    "FUN's nodes in view order, the blocks in the order of :blocks and each
  block's instructions before its terminator. Answers {:nodes [...] :starts
  {...}}: each node as {:node n :term? b}, and each label's first point."
    (def nodes @[])
    (def starts @{})
    (each b in (get fun :blocks)
      (put starts (get b :label) (length nodes))
      (each instr in (get b :instrs)
        (push nodes {:node instr :term? false}))
      (push nodes {:node (get b :term) :term? true}))
    {:nodes (freeze nodes) :starts (freeze starts)})

  (defn edges [view p]
    "The next facts of the node at point P of VIEW. A Return and a TailCall
  each lead to an exit of their own, and an Unreachable leads nowhere."
    (def {:node node :term? term?} (get (get view :nodes) p))
    (def starts (get view :starts))
    (def kind (get node :kind))
    (cond
      (not term?)
        (if (= kind :tail-call)
          [[p (+ p 1)] [p [:exit p]]]
          [[p (+ p 1)]])
      (= kind :return)
        [[p [:exit p]]]
      (= kind :jump)
        [[p (get starts (get node :target))]]
      (= kind :branch)
        [[p (get starts (get node :then-label))]
         [p (get starts (get node :else-label))]]
      (= kind :emit)
        [[p (get starts (get node :resume-label))]]
      (= kind :unreachable) []
      (error {:error :glean-error
              :reason :unknown-kind
              :kind kind
              :message (string "glean: no rule knows the terminator " kind)})))

  {:view view :edges edges})
