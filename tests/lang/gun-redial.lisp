(elle/epoch 15)
# audited: 2026-10-22
# std/gun keepers over loopback: :peers dials and redials with backoff, resync after a drop, and gun:close.
# lib/gun/mesh.md

(include-file "gun/check.lisp")
(include-file "gun/loopback.lisp")

(def hash-plugin (plugin "hash" (protect (import "plugin/hash"))))
(def random (plugin "random" (protect (import "plugin/random"))))
(def ws ((import "std/websocket") :hash hash-plugin :random random))
(def gun ((import "std/gun") :websocket ws))

# ============================================================================
# Helpers
# ============================================================================

(defn db-id [db]
  (get (get db :core) :id))

(defn lists? [db other]
  "True when db lists exactly the one peer other."
  (let [ids (gun:peers db)]
    (and (= (length ids) 1) (= (first ids) (db-id other)))))

(defn drop-peers [db]
  "Close every connection of db from its own side, as a peer that restarts
   would."
  (each p in (values (get (get db :core) :peers))
    (p:close)))

# ============================================================================
# Joining, a drop, and convergence
# ============================================================================

(defn reconnect []
  (let [a (open-db)
        [listener url port] (listen)
        stop-a (serve-on a listener)
        errs @[]]
    (gun:put (gun:get a "cfg") {:theme "dark"})
    (let [b (open-db :peers [url] :redial [0.1 0.4]
                     :on-error (fn [err] (push errs err)))
          seen @[]
          stop (gun:on (gun:get b "cfg") (fn [v] (push seen v)))
          note-a (gun:get a "note")
          note-b (gun:get b "note")]
      (assert (= (freeze seen) [nil])
              "a read before the first connection answers from local data")
      (eventually "B lists A" (fn [] (lists? b a)))
      (eventually "resync brings A's node to B, and calls on"
                  (fn [] (= (freeze seen) [nil {"theme" "dark"}])))
      (gun:put note-b {:from "b"} :ack 5)
      (assert (= (gun:once note-a) {"from" "b"}) "a write on B reaches A")

      # A goes away: its listener closes, so each redial is refused, and its
      # side of the connection closes. Each side then writes while the
      # other cannot hear it.
      (stop-a)
      (drop-peers a)
      (eventually "B sees A leave" (fn [] (empty? (gun:peers b))))
      (let [a-field (gun:get note-b "a")
            heard @[]
            stop-heard (gun:on a-field (fn [v] (push heard v)))]
        (gun:put (gun:get note-a "a") "x")
        (gun:put (gun:get note-b "b") "y")
        (eventually "a refused redial reaches :on-error"
                    (fn [] (not (empty? errs))))
        (let [stop-a (serve-on a (first (listen port)))]
          (eventually "B lists A again" (fn [] (lists? b a)))
          (let [both {"from" "b" "a" "x" "b" "y"}]
            (eventually "A gets B's write from while apart"
                        (fn [] (= (gun:once note-a) both)))
            (eventually "B gets A's write from while apart"
                        (fn [] (= (gun:once note-b) both))))
          # The trap: the merge lands before the on function is called, and the
          # forward between them yields, so the read of note-b can come first.
          (eventually "an on function hears the write that resync brought"
                      (fn [] (= (freeze heard) [nil "x"])))
          (stop-heard)
          (stop)
          (gun:close b)
          (stop-a)))
      (each e in errs
        (assert (= (get e :error) :io-error)
                (string "only refused dials reach :on-error: " e))))))

# ============================================================================
# Backoff
# ============================================================================

(defn backoff []
  (let [times @[]
        errs @[]
        b (open-db :peers ["ws://127.0.0.1:1/gun"] :redial [0.1 0.4]
                   :on-error (fn [err]
                               (push times (clock/monotonic))
                               (push errs err)))
        gap (fn [i] (- (get times (+ i 1)) (get times i)))]
    (eventually "six failed dials" (fn [] (>= (length times) 6)))
    (assert (>= (gap 0) 0.1) "the first wait is min")
    (assert (>= (gap 1) 0.2) "a failed dial doubles the wait")
    (assert (>= (gap 2) 0.4) "and doubles it again")
    (assert (>= (gap 3) 0.4) "the wait stays at max")
    # Uncapped, the fourth and fifth waits are 0.8 and 1.6 seconds.
    (assert (< (+ (gap 3) (gap 4)) 1.6) "the wait grows no further than max")
    (each e in errs
      (assert (= (get e :error) :io-error) "each refused dial is reported"))
    (gun:close b)
    (let [n (length times)]
      (ev/sleep 1)
      (assert (= (length times) n) "after close, the keeper dials no more"))))

# A keeper that kept its grown delay after a dial that succeeded would wait
# 3.2 seconds after the next drop, not 0.1.
(defn reset []
  (let [a (open-db)
        [listener url port] (listen)
        times @[]]
    (port/close listener)
    (let [b (open-db :peers [url] :redial [0.1 4]
                     :on-error (fn [err] (push times (clock/monotonic))))]
      (eventually "five failed dials" (fn [] (>= (length times) 5)))
      (let [stop-a (serve-on a (first (listen port)))]
        (eventually "B lists A once A listens" (fn [] (lists? b a)))
        (drop-peers a)
        (within 2 "after a dial that succeeded, a drop redials after min"
                (fn []
                  (until (fn [] (empty? (gun:peers b))))
                  (until (fn [] (lists? b a)))))
        (gun:close b)
        (stop-a)))))

# ============================================================================
# Closing a database
# ============================================================================

(defn closing []
  (let [srv (raw-server)
        b (open-db :peers [srv:url] :redial [0.1 0.4])
        [listener url-b _] (listen)
        stop-b (serve-on b listener)
        cfg (gun:get b "cfg")]
    (eventually "B lists the raw server" (fn [] (= (length (gun:peers b)) 1)))
    (gun:put cfg {:theme "dark"})
    (gun:close b)
    (assert (empty? (gun:peers b)) "close empties gun:peers at once")
    (let [n (srv:hellos)]
      (ev/sleep 0.5)
      (assert (= (srv:hellos) n) "no keeper dials again after close"))
    (let [conn (ws:connect url-b)]
      (raw-send conn {"hi" "late"})
      (within 5 "a peer that dials after close is closed"
              (fn []
                (assert (nil? (raw-recv conn)) "and its hello is not answered"))))
    (assert (empty? (gun:peers b)) "nor is it listed")
    (fails-with :closed (fn [] (gun:connect b srv:url)))
    (gun:close b)
    (gun:put cfg {:font "mono"})
    (assert (= (gun:once cfg) {"theme" "dark" "font" "mono"})
            "a closed database reads and writes locally")
    (stop-b)
    (srv:close)))

(reconnect)
(backoff)
(reset)
(closing)

(all-passed "gun-redial")
