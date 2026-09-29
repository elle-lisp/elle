(elle/epoch 12)
# audited: 2026-09-23
# EventManager: adding and removing handlers, broadcasts, and a handler that removes itself.
# docs/behaviors.md

(def process ((import "std/process")))


# ============================================================================
# EventManager
# ============================================================================

# ── 23. Add handler, notify, check state ─────────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:event-manager-start-link :name :events)

                   # A handler that collects events
                   (def @collector-mod
                     {:init (fn [_] @[])
                      :handle-event (fn [event state]
                                      (push state event)
                                      [:ok state])})

                   (let [ref (process:event-manager-add-handler :events collector-mod
                         nil)]
                     (process:event-manager-sync-notify :events :hello)
                     (process:event-manager-sync-notify :events :world)

                     # Check handlers list
                     (let [handlers (process:event-manager-which-handlers :events)]
                       (assert (= (length handlers) 1) "event: one handler"))

                     # Remove handler
                     (process:event-manager-remove-handler :events ref)
                     (let [handlers (process:event-manager-which-handlers :events)]
                       (assert (= (length handlers) 0) "event: handler removed"))))))
(println "  23. event manager: ok")


# ── 24. Multiple handlers receive same event ─────────────────────────

(process:start (fn []
                 (let [me (process:self)]
                   (process:event-manager-start-link :name :multi-events)

                   # Two handlers that forward events to us
                   (def @forwarder
                     (fn [tag]
                       {:init (fn [_] nil)
                        :handle-event (fn [event _state]
                                        (process:send me [tag event])
                                        [:ok nil])}))

                   (process:event-manager-add-handler :multi-events (forwarder :h1)
                   nil)
                   (process:event-manager-add-handler :multi-events (forwarder :h2)
                   nil)

                   (process:event-manager-sync-notify :multi-events :ping)

                   (def @got @||)
                   (match (process:recv)
                     [tag _event] (put got tag)
                     _ nil)
                   (match (process:recv)
                     [tag _event] (put got tag)
                     _ nil)
                   (assert (has? got :h1) "multi-event: h1 received")
                   (assert (has? got :h2) "multi-event: h2 received"))))
(println "  24. multiple event handlers: ok")


# ── 25. Handler self-removal via [:remove state] ─────────────────────

(process:start (fn []
                 (process:event-manager-start-link :name :remove-events)

                 # Handler that removes itself after seeing :done
                 (def @once-mod
                   {:init (fn [_] nil)
                    :handle-event (fn [event state]
                                    (if (= event :done)
                                      [:remove state]
                                      [:ok state]))})

                 (process:event-manager-add-handler :remove-events once-mod nil)
                 (assert (= 1
                            (length (process:event-manager-which-handlers :remove-events)))
                         "self-remove: handler present")

                 (process:event-manager-sync-notify :remove-events :keep)
                 (assert (= 1
                            (length (process:event-manager-which-handlers :remove-events)))
                         "self-remove: still present after :keep")

                 (process:event-manager-sync-notify :remove-events :done)
                 (assert (= 0
                            (length (process:event-manager-which-handlers :remove-events)))
                         "self-remove: removed after :done")))
(println "  25. handler self-removal: ok")

(println "event-manager: ok")
