(elle/epoch 12)
# Regression: an async io-backend dropped with an in-flight op
# — submitted but never reaped — must not corrupt the heap.
#
# io/submit with no following io/wait leaves the operation in
# flight; the kernel still owns a write pointer into the read
# buffer. When the throwaway backend goes out of scope each
# iteration it is dropped. Before the backend Drop learned to
# cancel+drain its pending ops — quiesce_pending in
# src/io/aio.rs — the kernel completed the read into the freed
# BufferPool slot: malloc unsorted-double-linked-list corruption.
# See docs/io.md "Backend teardown".

# Scratch file under the platform temp root (with-temp-dir honors TMPDIR and
# deletes the tree after, even on failure — no hardcoded paths, no litter).
(with-temp-dir dir
               (let [path (path/join dir "io-backend-drop-pending")]
                 (spit path "test")

                 # Submit a read on a throwaway backend and never io/wait. The
                 # backend value is dropped at the end of each iteration with the
                 # op still in flight.
                 (each i (range 64)
                   (let* [backend (io/backend :async)
                          port (port/open path :read)
                          f (fiber/new (fn [] (port/read-all port)) 512)]
                     (fiber/resume f)
                     (io/submit backend (fiber/value f))))

                 # Churn the allocator: a corrupted free-list left by a stray
                 # kernel write would trip here. Reaching the assertion means
                 # every teardown was clean.
                 (each i (range 4000)
                   (def junk @[1 2 3 4 5 6 7 8]))

                 (assert true
                         "io-backend dropped with an in-flight op did not corrupt the heap")))
