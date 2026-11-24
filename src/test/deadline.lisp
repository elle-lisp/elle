(elle/epoch 18)
# audited: 2026-11-23
## elle test — what the runner keeps of a form that missed its deadline: a
## photograph of every thread, the last line it printed, and its output.
## docs/test-runner.md
##
## A fragment of one module (see store.lisp).

(defn last-output-line [text]
  "The last non-empty line of `text`, or nil when it has none. Long lines are
   cut so one problem row stays one row."
  (if (= text nil)
    nil
    (let [lines (filter (fn [l] (> (length l) 0)) (string/split text "\n"))]
      (if (empty? lines)
        nil
        (let [l (get lines (- (length lines) 1))]
          (if (> (length l) 200) (concat (slice l 0 200) "…") l))))))

# A timed-out worker is ABANDONED, not killed — `sys/join` says so, because an
# OS thread cannot be safely killed. So at the moment the deadline is declared
# the wedged thread is still in THIS process, parked in whatever call stopped
# it, and its stack is still readable. Photograph every thread here, while that
# is true: after the run the process exits and the only account left is which
# budget ran out.
#
# This is the difference between a stack and a guess for a form that hangs on
# one machine and nowhere else. Re-running the file cannot stand in for it.
# The runner puts each form on its own worker thread, so a hang that needs that
# thread never reproduces under a plain run of the same file.
#
# stderr, because a terminal-only reader (a CI log) is who needs it, and the
# problem list above is already too narrow to hold a backtrace. Best-effort by
# construction: a box with no sampler prints nothing and the run is unaffected.
# `$PPID` inside `sh` is this process. Both samplers bound their own runtime
# (`sample` by its duration argument), so neither can wedge the run that is
# already in trouble.
#
# The photograph is printed whole. The runner cannot tell which thread wedged,
# and a cut at any length drops the threads the sampler happens to list last.
(defn photograph-threads []
  "A native backtrace of every thread in this process, or nil when the box has
   no sampler. `sample` is macOS's and always present there; `eu-stack` covers
   a Linux box that has elfutils."
  (let [[ok? proc] (protect (subprocess/exec "sh"
                              ["-c"
                               "sample $PPID 2 2>/dev/null || eu-stack -p $PPID 2>/dev/null"]))]
    (when ok?
      (let [[read-ok? out] (protect (string (port/read-all (get proc :stdout))))]
        (protect (subprocess/wait proc))
        (when (and read-ok? (> (length out) 0)) out)))))

(defn jit-frame-addrs [shot]
  "The unique `[0x…]` addresses on the photograph's `???` lines — the JIT
   frames the code map attributes and `jit/peek` reads."
  (let [@out @[]]
    (each line in (string/split shot "\n")
      (when (string/contains? line "???")
        (let [i (string/find line "[0x")]
          (when i
            (let [j (string/find line "]" i)]
              (when j
                (push out (slice line (+ i 1) j))))))))
    (distinct out)))

(defn note-timeout-stacks [c]
  "Print the wedged process's threads when a form misses its deadline."
  (when (= (get c :status) :timeout)
    (let [[ok? shot] (protect (photograph-threads))]
      (when (and ok? shot)
        (eprintln "── threads at the deadline ──────────────────────────────")
        (eprintln shot)
        # The photograph cannot symbolize JIT frames (anonymous Cranelift
        # mappings). The registry is their symbol table — process-global, so
        # it covers the wedged worker's compiles too. A sampled `???` address
        # resolves to the nearest preceding entry (docs/impl/jit.md).
        (let [[map-ok? jit-map] (protect (vm/query "jit/map" nil))]
          (when (and map-ok? (string? jit-map) (> (length jit-map) 0))
            (eprintln "── jit code map (addr name; match ??? frames to the nearest preceding addr) ──")
            (eprintln jit-map)))
        # The words each sampled JIT frame is parked on. The map names the
        # frame's function, and this shows the bytes its PC is executing.
        # Those bytes decide whether the wedge is IN the emitted code or in
        # what the core fetched (docs/impl/jit.md; on AArch64, `0x14000000` is
        # a branch to itself).
        (let [[a-ok? addrs] (protect (jit-frame-addrs shot))]
          (when (and a-ok? (> (length addrs) 0))
            (eprintln "── code around sampled jit frames (each line names its address) ──")
            (each a in (take 4 addrs)
              (let [[p-ok? words] (protect (vm/query "jit/peek" a))]
                (when (and p-ok? (string? words))
                  (eprintln (concat "pc " a ":"))
                  (eprintln words))))))
        (eprintln "── end threads ──────────────────────────────────────────"))))
  c)

# A timeout's reason names the budget that ran out — `join: deadline exceeded`
# — and says nothing about where the form was when it did. The last line the
# form printed says exactly that, so carry it in the reason. The problem list
# is what a terminal-only reader (a CI log) gets, and it should not need a
# query to name the call that hung. The full output stays in the assets.
(defn note-last-output [c cap]
  (if (= (get c :status) :timeout)
    (let [tail (let [e (last-output-line (get cap :stderr))]
                 (if e e (last-output-line (get cap :stdout))))]
      (if tail
        (put c :reason (concat (get c :reason) " · last output: " tail))
        c))
    c))

(defn slurp-partial [path]
  "What `path` holds, or an empty string when it holds nothing readable."
  (let [[ok? content] (protect (slurp path))]
    (if ok? content "")))

# Recover the output of a run that never came back.
#
# `capture-run` slurps and deletes the redirect files once the tiered call
# returns; a form killed by the join deadline never reaches that. Its output is
# the only account of which call it was in when the deadline struck, and the
# form wrote it before it wedged. So read the files here and record them
# against the timeout. Deleting them also keeps an abandoned worker from
# leaving its redirect pair in the scratch directory; the worker may still hold
# its end open, which POSIX allows.
(defn salvage-capture [result out-path err-path]
  (let [so (slurp-partial out-path)
        se (slurp-partial err-path)]
    (protect (file/delete out-path))
    (protect (file/delete err-path))
    (struct :result result :stdout so :stderr se)))
