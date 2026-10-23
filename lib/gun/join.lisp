(elle/epoch 15)
# audited: 2026-10-21
## How a std/gun database joins peers: serving, dialing, the keepers that redial, and close.
## lib/gun/mesh.md
## lib/gun/overview.md
##
## Loaded via:
##   (def join ((import "std/gun/join") :core core :peer peer :mesh mesh))
##
## Exports: {:check-redial :serve :connect :keep :close :peers}
##
## The database's :closed latch opens once, on close. A keeper waits for its
## delay on that latch, so close wakes every keeper at once.

(fn [&named core peer mesh]

  (defn check-redial [redial]
    "Raise :invalid-redial unless redial is [min max], two positive numbers
     with min no more than max."
    (unless (and (array? redial) (= (length redial) 2) (all? number? redial)
                 (< 0 (first redial)) (<= (first redial) (get redial 1)))
      (mesh:fail :invalid-redial (string ":redial is not [min max] in seconds: "
                 redial))))

  (defn serve [c listener]
    "Accept peers for c on listener until it closes."
    (peer:serve listener c:id (mesh:handlers c)))

  (defn dial [c url]
    (peer:dial url c:id c:wait (mesh:handlers c)))

  (defn connect [c url]
    "Dial the peer at url for c, and answer a function that closes the
     connection. Raises :closed once c has closed."
    (when (mesh:closed? c) (mesh:fail :closed "the database is closed"))
    (let [p (dial c url)]
      p:close))

  (defn pause [c seconds]
    "Wait seconds, or until c closes."
    (let [latch c:closed]
      (ev/timeout seconds
                  (fn []
                    (latch:wait)
                    true))))

  (defn keep [c url lo hi]
    "Start a keeper: a fiber that dials url for c until c closes. After a
     dial that succeeds, it waits for the peer to leave, and its delay goes
     back to lo. After a dial that fails, it reports the error. Then it waits
     its delay, which each failed dial doubles up to hi, and dials again."
    (peer:require-websocket)
    (ev/spawn (fn []
                (def @delay lo)
                (while (not (mesh:closed? c))
                  (let [[ok? p] (protect (dial c url))]
                    (cond
                      ok? (let [gone p:gone]
                            (gone:wait)
                            (assign delay lo)
                            (pause c delay))
                      (mesh:closed? c) nil
                      (begin
                        (core:report c p)
                        (pause c delay)
                        (assign delay (min hi (* 2 delay)))))))))
    nil)

  (defn close [c]
    "Close c: end every keeper, close every connected peer, and close each
     peer that registers after this. A second close does nothing."
    (let [latch c:closed]
      (latch:open))
    (each p in (values c:peers)
      (p:close))
    nil)

  (defn peers [c]
    "The database ids of the peers connected to c."
    (map (fn [p] p:id) (values c:peers)))

  {:check-redial check-redial
   :serve serve
   :connect connect
   :keep keep
   :close close
   :peers peers})
