(elle/epoch 14)
# audited: 2026-10-05
# The I/O leak dashboard: a leak rate for every probe whose drive reaches the I/O backend.
# docs/ratchet.md
# docs/impl/region/diagnostics.md
#
# oracle.lisp is the pure region dashboard. This file is split from it because
# io probes need axes it lacks — fixtures with setup and teardown, and the
# backend as a run dimension — and an io fixture failure must not void the
# pure dashboard's readings. Every probe here is read on the object count AND
# the region count in one drive: io machinery moves whole region entries
# between fibers, the scheduler and requests, so the region dimension is
# representable for every shape in the file. Every pin is a row in
# tests/ledger/plumb.lisp (lib/ratchet.md).
(def r ((import "std/ratchet")))
(def both [r:objects r:regions])

(defn rate-io [label probe]
  (r:rate label probe :on both))

(println "── plumb: io leak dashboard ──")

# ── The pumped round trip ─────────────────────────────────────────────
# A yielding io op, the whole round trip: ev/sleep is the clean shape
# (portless, nil result), so what the gauge sees is the scheduler pump's own
# per-op cost. The op suspends with an IoRequest, the pump reads a completion
# out of `(io/wait backend …)` and resumes the fiber with it, and every region
# on that path is released — the request's park retain at the resume, the
# completion array and the structs it carries at the pump's own
# `DecrefValueRegion`, both being one region (docs/impl/region/ctx.md).
# Measured with the B-invariance self-test, so a reading of 0 is a per-op rate
# rather than a per-block artifact.
(defn probe-io-yield [j]
  (ev/sleep 0))
(r:rate "io-yield ev/sleep" probe-io-yield :on both :block 200 :min 8 :max 80
        :stable true)

# ── The displaced io park ─────────────────────────────────────────────
# The three exits of a parked io op. Its `IoRequest` is the RUNTIME's value —
# the native built it and the body names it nowhere — so no continuation
# releases it and whatever ends the park owes that release
# (docs/impl/region/park.md). `io-drop` is the exit
# with no install at all, covered by the free-path discharge; `io-abort` and
# `io-refuse` each end the park by raising at the fiber's own suspension point.
# The three must stay together: `io-drop` removes the displacing install, so the
# gap between it and either displacing route isolates the install's owed release
# from the park itself. The per-install leak gauge is
# tests/impl/region-io-park.lisp and the guardfree face is
# tests/impl/region-io-park-uaf.lisp; these read the same shapes as rates.
(defn mk-io []
  (fiber/new (fn []
               (let [r (ev/sleep 10000)]
                 5)) |:io :error|))
(defn probe-io-drop [j]
  (let [f (mk-io)]
    (fiber/resume f)
    3))
(defn probe-io-abort [j]
  (let [f (mk-io)]
    (fiber/resume f)
    (fiber/abort f "no")))
(defn probe-io-refuse [j]
  (let [f (mk-io)]
    (fiber/resume f)
    (fiber/refuse f "no")))
(rate-io "io-drop" probe-io-drop)
(rate-io "io-abort" probe-io-abort)
(rate-io "io-refuse" probe-io-refuse)

# ── The abort the scheduler routes ────────────────────────────────────
# `io-abort` above ends a park through `fiber/abort` with no scheduler in the picture.
# `ev/abort` reaches the same primitive through the event loop, which then RECORDS what
# it did — a status record and a mark, both keyed by the fiber (docs/scheduler.md) — and
# CANCELS the operation the target was parked in, which leaves an entry holding its
# operands until the completion arrives (docs/impl/io-inflight.md). Either holder keeps
# the fiber, its closure, and the payload the abort delivered, per call, for as long as
# the loop runs.
#
# The two probes must stay together, and neither subsumes the other.
# `ev-abort` isolates the abort: it yields once so the target is genuinely
# parked in io, then aborts it and nothing else. `ev-timeout` is the shape the
# documentation tells a caller to bound work with, and it never blocks on io at
# all — its body wins at once — so it is the one that reads what an unreaped
# cancel costs a loop that gives the backend no chance to reap.
(defn probe-ev-abort [j]
  (let [f (ev/spawn (fn [] (ev/sleep 30)))]
    (ev/sleep 0)
    (ev/abort f)))
(defn probe-ev-timeout [j]
  (ev/timeout 30 (fn [] j)))
(rate-io "ev-abort" probe-ev-abort)
(rate-io "ev-timeout" probe-ev-timeout)

# ── The answer a completion BUILDS ────────────────────────────────────
# `io-yield ev/sleep` above answers with nil, so its completion builds nothing
# and the whole round trip costs the request's region alone. These three answer
# with a value the completion had to BUILD, because nothing could reserve it
# before the operation finished: a `subprocess` whose pid the spawn decides, and
# the bytes a `read-all` has only once the stream ends. Such a value is born in
# a region the completion owns and hands over as it becomes a value
# (docs/impl/io-inflight.md).
#
# All four must stay together. `io-yield` removes the built answer and reads
# the same 0, so the gap between it and any of these is the whole of what a
# completion's own region costs — one region and its objects per call, linear in
# the calls a program makes, and invisible to every other probe in this file.
#
# `subprocess-system` is one call whose completions build three times: the
# spawn's `subprocess`, and the answer each `read-all` on the child's two pipes
# ends with. Its two siblings take each of those shapes in its simplest form: the
# spawn with `:null` on all three streams, the `read-all` on a file. So neither
# of them reads a pipe, and neither hands over a region that carries a port.
#
# The two spawning probes run at a tenth of the block size the rest of the file
# uses: a block there is a block of CHILD PROCESSES, and the estimator's stopping
# rule converges on a deterministic shape in its minimum blocks either way.
(def built-dir (file/mktempdir))
(def built-path (string built-dir "/plumb-read-all"))
(spit built-path "plumb")
(defn probe-subprocess-exec [j]
  (let [p (subprocess/exec "/bin/sh" ["-c" ":"]
                           {:stdin :null :stdout :null :stderr :null})]
    (subprocess/wait p)))
(defn probe-read-all [j]
  (let [p (port/open built-path :read)
        s (port/read-all p)]
    (port/close p)
    (length s)))
(defn probe-subprocess-system [j]
  (get (subprocess/system "/bin/sh" ["-c" "echo plumb"]) :exit))
(r:rate "subprocess-exec" probe-subprocess-exec :on both :block 10 :max 40)
(rate-io "port-read-all" probe-read-all)
(r:rate "subprocess-system" probe-subprocess-system :on both :block 10 :max 40)
(delete-file built-path)
(delete-directory built-dir)

# The double-release counter, read once after the last probe: the io backend
# moves whole region entries between fibers, the scheduler and requests, so a
# release that runs twice is as reachable here as anywhere, and this file's
# probes are not under the oracle's reading (docs/impl/region/diagnostics.md).
(r:read "over-free" :releases (arena/over-frees))

(println "plumb: ok")
