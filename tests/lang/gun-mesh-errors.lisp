(elle/epoch 15)
# audited: 2026-10-21
# std/gun errors between peers over loopback WebSocket: a subscriber's error, a bad message, and sends that interleave.
# lib/gun/mesh.md
# lib/gun.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))
(def state ((import "std/gun/state")))

# ============================================================================
# Errors
# ============================================================================

(defn errors []
  # A local subscriber's error leaves the put before it waits for its ack.
  # The wait must not outlive the put. Nothing in the API shows a wait that
  # stayed, so this reads the database's table of waits.
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        field (gun:get (gun:get b "raise") "v")
        stop (gun:on field
                     (fn [v]
                       (when v
                         (error {:error :subscriber
                                 :reason :boom
                                 :message "boom"}))))
        [ok? err] (protect (gun:put field 1 :ack 5))]
    (assert (and (not ok?) (= (get err :reason) :boom))
            "a local subscriber's error leaves a put with :ack")
    (assert (empty? (get (get b :core) :pending))
            "and the put leaves no wait for its ack behind")
    (stop)
    (hang-up)
    (srv:close))

  (let [errs @[]
        a (open-db)
        b (open-db :on-error (fn [err] (push errs err)))
        srv (serve a)
        hang-up (gun:connect b srv:url)
        field-b (gun:get (gun:get b "boom") "v")
        stop (gun:on field-b
                     (fn [v]
                       (when (= v 1)
                         (error {:error :subscriber
                                 :reason :boom
                                 :message "boom"}))))]
    (gun:put (gun:get (gun:get a "boom") "v") 1 :ack 5)
    (eventually "a subscriber's error in a remote write reaches :on-error"
                (fn [] (= (map (fn [e] (get e :reason)) errs) [:boom])))
    (gun:put (gun:get (gun:get a "boom") "v") 2 :ack 5)
    (assert (= (gun:once field-b) 2) "and the next write still arrives")
    (stop)
    (hang-up)
    (srv:close))

  (let [errs @[]
        a (open-db :on-error (fn [err] (push errs err)))
        srv (serve a)
        raw (raw-client srv:url)
        st (state:pack (- (wall-ms) 1000) 0)]
    (raw:send "not json")
    (raw:send {"id" "r1" "put" {"raw" {"v" [st "s:ok"]}}})
    (eventually "the valid put is acked" (fn [] (ack-of raw:heard "r1")))
    (assert (= (gun:once (gun:get (gun:get a "raw") "v")) "ok")
            "a bad message does not stop the next one")
    (assert (= (map (fn [e] (get e :reason)) errs) [:bad-message])
            "the bad message reaches :on-error")
    (raw:close)
    (srv:close)))

# ============================================================================
# Concurrent sends
# ============================================================================

# The trap: small frames never tear on loopback. The std/websocket transport
# lets two senders interleave only once a write is partial, so the values
# are large enough to fill the socket buffer.
(defn concurrent-sends []
  (let [a (open-db)
        b (open-db)
        srv (serve a)
        hang-up (gun:connect b srv:url)
        on-a (gun:get (gun:get a "burst") "v")
        on-b (gun:get (gun:get b "burst") "v")
        big (string/repeat "x" 300000)
        writer (fn [tag]
                 (each i in (range 8)
                   (gun:put on-a (string tag "-" i big))))]
    (assert (nil? (gun:once on-b)) "burst starts empty")
    (ev/join [(ev/spawn (fn [] (writer "x"))) (ev/spawn (fn [] (writer "y")))
              (ev/spawn (fn [] (writer "z")))])
    (gun:put on-a "final" :ack 20)
    (eventually "B ends on A's last write" (fn [] (= (gun:once on-b) "final")))
    (hang-up)
    (srv:close)))

(errors)
(concurrent-sends)

(all-passed "gun-mesh-errors")
