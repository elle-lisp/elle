(elle/epoch 15)
# audited: 2026-10-21
# std/gun stores and closing: the memory store, loading ahead of the clock, the store lock, close, and the mesh options.
# lib/gun.md
# lib/gun/mesh.md

(include-file "gun/check.lisp")

(def gun ((import "std/gun")))
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def stores ((import "std/gun/store") :state state :value value))

# ============================================================================
# Stores
# ============================================================================

(let [s (stores:memory-store)]
  (assert (nil? (s:read "nothing"))
          "a memory store reads nil for an absent soul")
  (s:write "n" {"k" [5 "a"] "l" [5 (value:link "m")]})
  (s:write "n" {"k" [6 nil]})
  (assert (= (s:read "n") {"k" [6 nil] "l" [5 (value:link "m")]})
          "a write replaces the fields it names and keeps the rest"))

(let [s (gun:memory-store)]
  (gun:put (gun:get (open-db :store s :clock (frozen-clock 1000)) "cfg")
           {:theme "dark"})
  (assert (= (gun:once (gun:get (open-db :store s :clock (frozen-clock 1000))
                                "cfg")) {"theme" "dark"})
          "a second database on the store sees the first one's writes"))

# A store written by a clock that ran ahead loads without :future, and the
# next local write still wins, because loading observes the stored states.
(let [s (gun:memory-store)]
  (gun:put (gun:get (open-db :store s :clock (frozen-clock 5000)) "cfg")
           {:theme "dark"})
  (let [behind (open-db :store s :clock (frozen-clock 1000))
        cfg (gun:get behind "cfg")]
    (assert (= (gun:once cfg) {"theme" "dark"})
            "a store ahead of the clock loads")
    (gun:put cfg {:theme "light"})
    (assert (= (gun:once cfg) {"theme" "light"}) "and the next write wins")))

# Two fibers put one field through a store whose first write sleeps. The
# lock holds the second put until the first write returns, so the store
# receives the field's writes in state order. Without the lock, the second
# put merges and stores its newer write while the first sleeps, and then the
# first, older write lands over it: the store keeps a value that memory
# has already replaced.
(let [inner (gun:memory-store)
      @delays [0.05]
      slow {:read (fn [soul] (inner:read soul))
            :write (fn [soul fields]
                     (unless (empty? delays)
                       (let [d (first delays)]
                         (assign delays (slice delays 1))
                         (ev/sleep d)))
                     (inner:write soul fields))}
      field (gun:get (gun:get (open-db :store slow) "race") "v")]
  (ev/join [(ev/spawn (fn [] (gun:put field "first")))
            (ev/spawn (fn [] (gun:put field "second")))])
  (assert (= (gun:once (gun:get (gun:get (open-db :store inner) "race") "v"))
             (gun:once field))
          "a slow store write cannot let an older write land last"))

(fails-with :redis-not-configured (fn [] (gun:redis-store :prefix "unused")))

# ============================================================================
# Closing, and the options of the mesh
# ============================================================================

(let [db (open-db :clock (frozen-clock 1000))
      cfg (gun:get db "cfg")]
  (gun:put cfg {:theme "dark"})
  (gun:close db)
  (gun:close db)
  (assert (empty? (gun:peers db)) "a closed database lists no peer")
  (gun:put cfg {:font "mono"})
  (assert (= (gun:once cfg) {"theme" "dark" "font" "mono"})
          "a closed database reads and writes locally"))

(each r in [[5 1] [0 1] [-1 1] [1 -1] [1] [1 2 3] [1 "a"] "fast" 1]
  (fails-with :invalid-redial (fn [] (gun:open :redial r))))
(gun:close (open-db :redial [0.5 0.5]))
(fails-with :mesh-not-configured (fn []
                                   (open-db :peers ["ws://127.0.0.1:1/gun"])))

(all-passed "gun-store")
