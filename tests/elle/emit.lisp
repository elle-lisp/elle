# ── emit special form tests ───────────────────────────────────────────
#
# (emit <signal> <value>) is a special form when the first argument is
# a compile-time keyword or keyword set. Signal bits are extracted
# at analysis time and encoded in the bytecode instruction.

# ── Basic emit with keywords ─────────────────────────────────────────

# (emit :yield val) behaves like old (yield val)
(let ([f (fiber/new (fn [] (let ([r (emit :yield 42)]) (+ r 10))) |:yield|)])
  (assert (= (fiber/resume f) 42) "emit :yield produces yield value")
  (assert (= (fiber/status f) :paused) "emit :yield pauses fiber")
  (assert (= (fiber/resume f 5) 15) "emit :yield resumes: 5 + 10 = 15"))

# (emit :yield) — no implicit nil form (emit requires 2 args)
# Use (yield) for the 0-arg form.

# (emit :error val) emits an error signal
(let ([f (fiber/new (fn [] (emit :error :boom)) |:error|)])
  (let ([result (fiber/resume f)])
    (assert (= (fiber/status f) :paused) "emit :error pauses")
    (assert (= (fiber/value f) :boom) "emit :error payload is :boom")))

# ── Set argument ─────────────────────────────────────────────────────

# (emit |:yield :io| val) emits compound signal bits
(let ([f (fiber/new (fn [] (emit |:yield :io| :data)) |:yield :io|)])
  (let ([result (fiber/resume f)])
    (assert (= result :data) "emit set: value received")
    (let ([bits (fiber/bits f)])
      # bits should contain both :yield (2) and :io (512)
      (assert (not (= bits 0)) "emit set: non-zero signal bits")
      (assert (= (bit/and bits 2) 2) "emit set: :yield bit present")
      (assert (= (bit/and bits 512) 512) "emit set: :io bit present"))))

# ── yield still works (it's still a special form for now) ────────────

(let ([f (fiber/new (fn [] (yield 10) (yield 20) 30) |:yield|)])
  (assert (= (fiber/resume f) 10) "yield works: first")
  (assert (= (fiber/resume f) 20) "yield works: second")
  (assert (= (fiber/resume f) 30) "yield works: final"))

# (yield) with no args yields nil
(let ([f (fiber/new (fn [] (yield) 42) |:yield|)])
  (assert (= (fiber/resume f) nil) "yield no-arg yields nil")
  (assert (= (fiber/resume f) 42) "yield no-arg: final value"))

# ── Dynamic emit still works via primitive fallback ──────────────────

# (emit 2 val) falls through to prim_emit (runtime, not special form)
(let ([f (fiber/new (fn [] (emit 2 :dynamic)) |:yield|)])
  (assert (= (fiber/resume f) :dynamic) "dynamic emit still works"))

# ── Resume value flows back through emit ─────────────────────────────

(let ([f (fiber/new (fn [] (let ([x (emit :yield 1)]) (+ x 10))) |:yield|)])
  (assert (= (fiber/resume f) 1) "first emit value")
  (assert (= (fiber/resume f 5) 15) "resume value 5 + 10 = 15"))

# ── Multiple emits in sequence ───────────────────────────────────────

(let ([f (fiber/new
           (fn []
             (let ([a (emit :yield :first)])
               (let ([b (emit :yield :second)])
                 (list a b))))
           |:yield|)])
  (assert (= (fiber/resume f) :first) "multi-emit: first")
  (assert (= (fiber/resume f :a) :second) "multi-emit: second")
  (assert (= (fiber/resume f :b) (list :a :b)) "multi-emit: collected"))

# ── emit interacts correctly with capability denial ──────────────────

# A fiber with :deny |:error| can still (emit :yield val) because
# the emit instruction itself doesn't go through call_inner
(let ([f (fiber/new (fn [] (emit :yield 42)) |:yield| :deny |:error|)])
  (assert (= (fiber/resume f) 42) "emit :yield works despite :deny |:error|"))
