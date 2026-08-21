(elle/epoch 12)
# Regression for sendable parameters + stdio ports across os/spawn
# (src/value/send.rs). A parameter is fiber-local state; before this change the
# serializer rejected ANY parameter with "Cannot send parameter", which meant a
# closure that merely *referenced* a parameter — e.g. anything calling `println`,
# which closes over `*stdout*` — could not be shipped to a worker at all.
#
# Now a parameter is sendable iff its default + traits are sendable, with its
# global id preserved (resolution is by id), and the stdin/stdout/stderr ports
# are reconstructed fresh in the worker. File/socket ports stay unsendable.
#
# Counter-factual (pre-change): every os/spawn below raised
# {:error :thread-error :message "...Cannot send parameter"}.

# 1) A plain parameter survives the boundary: its default is readable, a
#    `parameterize` inside the worker rebinds it, and the binding is restored.
(def p (parameter 42))
(let [r (os/join (os/spawn-vm (fn []
                                [(p)
                                 (parameterize ((p 99))
                                   (p)) (p)])))]
  (assert (= r [42 99 42])
          (string "expected [42 99 42] (default/rebound/restored), got " r)))

# 2) A closure that closes over a parameter through a captured helper still
#    sends — the parameter is reached transitively, not just at top level.
(defn read-p []
  (p))
(let [r (os/join (os/spawn-vm (fn [] (read-p))))]
  (assert (= r 42) (string "captured-helper parameter read expected 42, got " r)))

# 3) The standard streams are parameters whose default is a stdio port; sending
#    such a closure exercises BOTH parameter-send and stdio-port reconstruction.
#    (No I/O here — a bare os/spawn worker has no scheduler; this only checks the
#    values cross the boundary intact.)
(let [r (os/join (os/spawn-vm (fn [] [(parameter? *stdout*) (port? (*stdout*))])))]
  (assert (= r [true true])
          (string "*stdout* must cross as a parameter holding a port, got " r)))

# 4) A file/socket port is NOT sendable — its fd is owned and meaningless in
#    another VM. The spawn fails loudly with a clear message (protect catches it).
(def scratch (file/mktempdir))
(def neg-file (path/join scratch "send-param-neg.out"))
(def fp (port/open neg-file :write))
(let [r (protect (os/spawn-vm (fn [] (port? fp))))]
  (assert (not (get r 0)) "spawning a closure capturing a file port must fail")
  (assert (string/contains? (get (get r 1) :message)
                            "Cannot send a file or socket port")
          (string "expected the file-port rejection message, got " (get r 1))))
(port/close fp)
(file/delete-dir-all scratch)

(println "send-param tests passed")
