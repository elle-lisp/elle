(elle/epoch 12)
## tests/elle/process-accept-close.lisp — closing a listener ends a process
## parked in accept.
##
## The shape is tests/elle/process-io.lisp case 21 with the h2 machinery
## removed: process A holds a listener, process B parks in tcp/accept on it,
## and A closes the listener and exits while B is still parked. The accept
## must complete with an error, B must die, and the scheduler must return.
##
## The thread-pool backend cannot lean on shutdown(2) to wake the parked
## accept: shutdown of a LISTENING socket wakes an accept only on Linux —
## macOS and the BSDs return ENOTCONN and wake nothing. The close must reach
## the worker through the operation's stop pipe. If that wake is lost, the
## accept never completes, B never dies, and process:start parks forever —
## this file then times out under the runner's deadline.
##
## The Rust half of the pin is `closing_a_listener_ends_its_parked_pool_accept`
## (src/io/aio/tests/net.rs), which drives the pool backend directly.

(def process ((import "std/process")))

(println "tests/elle/process-accept-close.lisp:")

(def @server-ended false)

(process:start (fn []
                 (let [listener (tcp/listen "127.0.0.1" 0)]
                   # Server process parks in accept; nothing ever connects.
                   (process:spawn (fn []
                                    (protect (tcp/accept listener))
                                    (assign server-ended true)))
                   # Yield so the server runs and parks in the accept before
                   # the close: each yield gives the scheduler a full turn.
                   (process:self)
                   (process:self)
                   (process:self)
                   (port/close listener))))

(assert server-ended "server process ended after its listener closed")
(println "  1. close wakes a process parked in accept: ok")

(println "")
(println "tests/elle/process-accept-close.lisp: all tests passed")
