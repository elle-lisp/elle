(elle/epoch 12)
# Counterfactual for the strand a `Fresh` io op leaves on its own region.
#
# `port/read` and its siblings declare `RegionEffect::Fresh` and mint ONE
# region per call, holding both the IoRequest the native returns and the read
# buffer the completion fills in place. Two references stand on that region and
# they answer to different consumers:
#
#   - the `Fresh` mint, consumed by the release of the value the suspend hands
#     back — the request at the park, the buffer at the resume;
#   - the `SuspendEscape` retain the park takes so the scheduler can read the
#     request out of `fiber.signal`, which the install that displaces the park
#     owes (docs/impl/region/owner.md § "A payload the RUNTIME built is
#     released by the install that displaces it").
#
# An install that releases only the first leaves the second standing, and the
# region survives with its buffer and its request — one region and two objects
# per read, so unbounded in any program that reads in a loop. A socket reader
# pays it per frame, which is what makes it a server's leak rather than a
# script's.

(def reads 400)

# ── Leak bound: a sized read strands no region ────────────────────────
# The gauge is sampled by the program around a fixed window of reads on one
# open port. The port is rewound by re-opening rather than by seeking, so the
# window measures the reads alone.
(defn read-region-churn [p n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (port/read p 4)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn read-object-churn [p n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (port/read p 4)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn make-payload [n]
  (let [@parts @[]]
    (def @i 0)
    (while (%lt i n)
      (push parts "abcd")
      (assign i (%add i 1)))
    (apply concat (freeze parts))))

(with-temp-dir tmp
               (let [path (string tmp "/reads.bin")]
                 (file/write path (make-payload (* reads 4)))
                 (let [p (port/open path :read)]
                   (let [d (read-region-churn p reads)]
                     (assert (%lt d 40)
                             (string "io request region strand: " reads
                                     " file reads grew the region count by " d
                                     " (must stay bounded — Rule 8)")))
                   (port/close p))
                 (let [p (port/open path :read)]
                   (let [d (read-object-churn p reads)]
                     (assert (%lt d 40)
                             (string "io request object strand: " reads
                                     " file reads grew the live count by " d
                                     " (must stay bounded — Rule 8)")))
                   (port/close p))))

# ── The same bound on a socket, the shape a server loop runs ──────────
# A loopback pair is read one 4-byte chunk at a time, so the window counts
# reads and not bytes. The writer sends the whole payload up front: a socket
# whose peer writes per read would measure the write path too.
(let* [listener (tcp/listen "127.0.0.1" 0)
       lpath (port/path listener)
       lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
       af (ev/spawn (fn [] (tcp/accept listener)))
       cli (tcp/connect "127.0.0.1" lport)
       srv (ev/join af)]
  (port/write cli (bytes (make-payload (* reads 4))))
  (let [d (read-region-churn srv reads)]
    (assert (%lt d 40)
            (string "io request region strand: " reads
                    " socket reads grew the region count by " d
                    " (must stay bounded — Rule 8)")))
  (port/close cli)
  (port/close srv)
  (port/close listener))

# ── Correctness: the buffer outlives the install that ends the park ───
# The counter-factual for the bound above going too far. The install releases
# the suspend retain, not the buffer: the `Fresh` mint is a separate reference
# and the resume value's own holders are separate again. Every chunk read below
# is held past the reads that follow it, so a release that took one reference
# too many frees a chunk under this array and the comparison reads garbage.
(with-temp-dir tmp
               (let [path (string tmp "/held.bin")]
                 (file/write path "0123456789abcdefghij")
                 (let [p (port/open path :read)
                       @chunks @[]]
                   (def @i 0)
                   (while (%lt i 5)
                     (push chunks (port/read p 4))
                     (assign i (%add i 1)))
                   (port/close p)
                   (assert (= (get chunks 0) "0123")
                           (string "first chunk survived four later reads: "
                                   (get chunks 0)))
                   (assert (= (get chunks 4) "ghij")
                           (string "last chunk read back: " (get chunks 4)))
                   (assert (= (apply concat (freeze chunks))
                              "0123456789abcdefghij")
                           "every held chunk survived the reads that followed it"))))
