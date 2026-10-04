(elle/epoch 14)
# audited: 2026-10-03
# An async io-backend dropped with an operation still in flight leaves the heap intact.
# docs/io.md
#
# io/submit with no following io/wait leaves the operation in flight, and the
# kernel still owns a write pointer into the read buffer. The counter-factual:
# a drop that frees the BufferPool slot without cancelling and draining its
# pending operations (quiesce_pending in src/io/aio/drain.rs) lets the kernel
# complete the read into freed memory. malloc then aborts on a corrupted list.

(with-temp-dir dir
               (let [path (path/join dir "io-backend-drop-pending")]
                 (spit path "test")

                 # Submit a read on a throwaway backend and never io/wait. The
                 # backend value is dropped at the end of each iteration with the
                 # op still in flight. The status read keeps f waiting on its
                 # request through the submit. A submit after f's release raises
                 # :state-error.
                 (each i (range 64)
                   (let* [backend (io/backend :async)
                          port (port/open path :read)
                          f (fiber/new (fn [] (port/read-all port)) 512)]
                     (fiber/resume f)
                     (io/submit backend (fiber/value f))
                     (assert (= (fiber/status f) :paused)
                             "the submit leaves the fiber waiting on its request")))

                 # Churn the allocator: a corrupted free-list left by a stray
                 # kernel write would trip here. Reaching the assertion means
                 # every teardown was clean.
                 (each i (range 4000)
                   (def junk @[1 2 3 4 5 6 7 8]))

                 (assert true
                         "io-backend dropped with an in-flight op did not corrupt the heap")))
