(elle/epoch 13)
# audited: 2026-09-29
# The process primitives: each one yields a command to the scheduler, so it runs only inside a process.
# docs/processes.md

(defn send [pid msg]
  (yield [:send pid msg]))
(defn recv []
  (yield [:recv]))
(defn recv-match [pred]
  (yield [:recv-match pred]))
(defn recv-timeout [ticks]
  (yield [:recv-timeout ticks]))
(defn self []
  (yield [:self]))
(defn process-fiber [closure]
  "The fiber a process runs closure in. The spawn primitives build it in the
   calling fiber, so the new process withholds what its spawner withholds. The
   mask names each signal the process scheduler serves, and each bit a
   capability denial raises, which the scheduler refuses
   (docs/process-scheduler.md)."
  (fiber/new closure
             |:yield :error :fuel :io :exec :wait :ffi :gpu :os-signal :fs|))
(defn spawn [closure]
  (yield [:spawn (process-fiber closure)]))
(defn spawn-link [closure]
  (yield [:spawn-link (process-fiber closure)]))
(defn spawn-monitor [closure]
  (yield [:spawn-monitor (process-fiber closure)]))
(defn link [pid]
  "Link to pid. Raises {:error :noproc} when pid has exited and the caller
   neither traps exits nor was linked to it."
  (when (= (yield [:link pid]) :noproc)
    (error {:error :noproc :message (string "link: process " pid " has exited")}))
  :ok)
(defn unlink [pid]
  (yield [:unlink pid]))
(defn monitor [pid]
  (yield [:monitor pid]))
(defn demonitor [ref &named flush]
  "Stop the monitor ref. With :flush true, also drop its :DOWN from the mailbox."
  (yield [:demonitor ref flush]))
(defn trap-exit [flag]
  (yield [:trap-exit flag]))
(defn exit [pid reason]
  (yield [:exit pid reason]))
(defn register [name]
  (yield [:register name]))
(defn unregister [name]
  (yield [:unregister name]))
(defn whereis [name]
  (yield [:whereis name]))
(defn send-named [name msg]
  (yield [:send-named name msg]))
(defn send-after [ticks pid msg]
  (yield [:send-after ticks pid msg]))
(defn now []
  "The scheduler's clock, in ticks."
  (yield [:now]))
(defn cancel-timer [ref]
  (yield [:cancel-timer ref]))
(defn put-dict [key val]
  (yield [:put-dict key val]))
(defn get-dict [key]
  (yield [:get-dict key]))
(defn erase-dict [key]
  (yield [:erase-dict key]))

(fn []
  {:send send
   :recv recv
   :recv-match recv-match
   :recv-timeout recv-timeout
   :self self
   :process-fiber process-fiber
   :spawn spawn
   :spawn-link spawn-link
   :spawn-monitor spawn-monitor
   :link link
   :unlink unlink
   :monitor monitor
   :demonitor demonitor
   :trap-exit trap-exit
   :exit exit
   :register register
   :unregister unregister
   :whereis whereis
   :send-named send-named
   :send-after send-after
   :now now
   :cancel-timer cancel-timer
   :put-dict put-dict
   :get-dict get-dict
   :erase-dict erase-dict})
