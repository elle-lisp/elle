(elle/epoch 12)
# audited: 2026-09-23
# Erlang-style processes on fibers: the scheduler, its primitives, and the OTP-shaped behaviors.
# lib/process.md
# docs/processes.md
# docs/behaviors.md
#
# Loaded via: (def process ((import "std/process")))
# Usage:      (process:start (fn [] (process:send (process:self) :hello) (process:recv)))

(def primitives ((import "std/process/primitives")))
(def scheduler ((import "std/process/scheduler")))
(def genserver ((import "std/process/genserver") primitives))
(def task ((import "std/process/task") primitives genserver))
(def supervisor ((import "std/process/supervisor") primitives genserver))
(def event ((import "std/process/event") genserver))

(def exports
  (reduce merge
          {:gen-server-start-link genserver:gen-server-start-link
           :gen-server-call genserver:gen-server-call
           :gen-server-cast genserver:gen-server-cast
           :gen-server-stop genserver:gen-server-stop
           :gen-server-reply genserver:gen-server-reply
           :actor-start-link genserver:actor-start-link
           :actor-get genserver:actor-get
           :actor-update genserver:actor-update
           :actor-cast genserver:actor-cast}
          [primitives scheduler task supervisor event]))

(fn [] exports)
