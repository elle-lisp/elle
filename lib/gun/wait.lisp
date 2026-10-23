(elle/epoch 15)
# audited: 2026-10-21
## A std/gun wait: the peers a get or a put went to, and the latch that opens when they answered.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def wait ((import "std/gun/wait")))
##
## Exports: {:open-wait :settle :await}
##
## A wait lives in the database's :pending table under the id of its
## message. Its latch opens when every peer answered or left (a get), or
## when the first one acked (a put).

(fn []

  (def sync ((import "std/sync")))

  (defn open-wait [c id targets until]
    "Wait for the peers in targets to answer the message id. until is :all
     for a get and :first for a put."
    (let [waiting @{}]
      (each p in targets
        (put waiting p:key true))
      (put c:pending id
           {:waiting waiting
            :until until
            :latch (sync:make-latch)
            :first (box nil)})))

  (defn settle [c id key answer]
    "Record that the peer with key answered the message id, with answer."
    (when-let [w (get c:pending id)] (del w:waiting key)
              (when (and answer (nil? (unbox w:first))) (rebox w:first answer))
              (when (or (empty? w:waiting) (and answer (= w:until :first)))
                (let [latch w:latch]
                  (latch:open)))))

  (defn await [c id seconds]
    "Wait for the message id to settle, for seconds at most, and answer the
     first answer it recorded, or nil."
    (let [w (get c:pending id)
          latch w:latch]
      (defer
        (del c:pending id)
        (unless (empty? w:waiting)
          (ev/timeout seconds
                      (fn []
                        (latch:wait)
                        true)))
        (unbox w:first))))

  {:open-wait open-wait :settle settle :await await})
