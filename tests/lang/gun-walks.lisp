(elle/epoch 15)
# audited: 2026-10-21
# std/gun walks over loopback WebSocket: a subscription, a put and a map that fetch what the peers hold.
# lib/gun/mesh.md
# lib/gun/chains.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))

(defn subscriptions []
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        seen @[]
        stop (gun:on (gun:get (gun:get b "live") "v") (fn [v] (push seen v)))]
    (gun:put (gun:get (gun:get a "live") "v") 1 :ack 5)
    (gun:put (gun:get (gun:get a "live") "v") 2 :ack 5)
    (eventually "B's subscription sees A's writes"
                (fn [] (= (freeze seen) [nil 1 2])))
    (stop)
    (hang-up)
    (srv:close)))

# B's subscription walks to carl only after A links it, and B has never
# fetched carl. A walk that read only local data would answer nil for ever.
(defn notify-fetches []
  (let [a (open-db)
        b (open-db)]
    (gun:put (gun:get a "carl") {:name "Carl"})
    (gun:put (gun:get a "alice") {:name "Alice"})
    (let [srv (serve a)
          hang-up (gun:connect b srv:url)
          seen @[]
          stop (gun:on (gun:get (gun:get (gun:get b "alice") "boss") "name")
                       (fn [v] (push seen v)))]
      (gun:put (gun:get (gun:get a "alice") "boss") (gun:link "carl") :ack 5)
      (eventually "the subscription fetches the newly linked node"
                  (fn [] (= (freeze seen) [nil "Carl"])))
      (stop)
      (hang-up)
      (srv:close))))

# A put that planned on local data alone would write a derived node for
# alice's pet and relink pet to it, away from rex.
(defn put-walks-the-mesh []
  (let [a (open-db)
        b (open-db)]
    (gun:put (gun:get a "rex") {:kind "dog"})
    (gun:put (gun:get (gun:get a "users") "alice") {:pet (gun:link "rex")})
    (let [srv (serve a)
          hang-up (gun:connect b srv:url)]
      (gun:put (gun:get (gun:get (gun:get (gun:get b "users") "alice") "pet")
                        "name") "Rex" :ack 5)
      (eventually "the put writes the node that the mesh links"
                  (fn []
                    (= (gun:once (gun:get a "rex")) {"kind" "dog" "name" "Rex"})))
      (assert (= (gun:link-soul (get (gun:once (gun:get (gun:get a "users")
                                     "alice")) "pet")) "rex")
              "and keeps the link")
      (hang-up)
      (srv:close))))

# B opens on a store that another database filled, so B has loaded nothing,
# and its only peer records every get. A map that fetched its members one by
# one would send three gets and still answer the same struct.
(defn map-fetches-once []
  (let [store (gun:memory-store)
        filler (gun:open :store store)]
    (gun:put (gun:get filler "crew") {:a {:n 1} :b {:n 2} :c {:n 3}})
    (let [b (open-db :store store)
          spy (raw-server)
          hang-up (gun:connect b spy:url)
          members ["crew/a" "crew/b" "crew/c"]]
      (assert (= (gun:once (gun:get (gun:map (gun:get b "crew")) "n"))
                 {"a" 1 "b" 2 "c" 3}) "the map reads every member")
      (assert (= (length (gets-naming spy:heard "crew")) 1)
              "one get fetches the collection")
      (each soul in members
        (assert (= (length (gets-naming spy:heard soul)) 1)
                (string "one get names " soul)))
      (assert (= (gets-naming spy:heard "crew/a")
                 (gets-naming spy:heard "crew/c"))
              "and it is one get for all three members")
      (hang-up)
      (spy:close))))

(subscriptions)
(notify-fetches)
(put-walks-the-mesh)
(map-fetches-once)

(all-passed "gun-walks")
