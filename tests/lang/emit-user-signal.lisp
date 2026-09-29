(elle/epoch 12)
# ── A literal emit carries a user signal's bit whole ──────────────────
#
# `(emit :keyword payload)` with a literal keyword compiles to the `Emit`
# instruction, which encodes the signal bits as an operand. User signals
# live at bits 32-63 (docs/signals/protocol.md), so the operand must be
# 64 bits wide. A narrower operand truncates every user bit to zero, and
# an emit of empty bits is indistinguishable from a normal return.
#
# The trap: nothing about a truncated emit looks like a failure at the
# emit site. The payload still reaches the parent, so a test that only
# checks the payload passes on both the correct and the broken encoding.
# The status, the bits, and the resumed body are what tell them apart.

(signal :user_sig_893)
(def user-bit (get (signals) :user_sig_893))
(def user-bits (bit/shift-left 1 user-bit))

# The registry allocates user signals from bit 32 upward. Below 32 the
# assertions further down would still hold under a 32-bit operand, so
# this is what makes them a test of the wide encoding.
(assert (>= user-bit 32) "a user signal is allocated at bit 32 or above")

# ── A literal emit suspends and resumes ──────────────────────────────

(let [f (fiber/new (fn []
                     (let [r (emit :user_sig_893 {:p 2})]
                       [:resumed r])) |:user_sig_893|)]
  (assert (= (fiber/resume f) {:p 2}) "literal emit delivers its payload")
  (assert (= (fiber/status f) :paused)
          "literal emit of a user signal suspends the fiber")
  (assert (= (fiber/bits f) user-bits)
          "literal emit reports the user signal's bit")
  (assert (= (fiber/resume f "MEDIATED") [:resumed "MEDIATED"])
          "the resume value is the result of the literal emit"))

# ── The literal path agrees with the dynamic path ────────────────────
#
# A non-literal first argument falls through to the runtime primitive,
# which reads the registry and keeps all 64 bits. Both paths emit the
# same signal, so both must report the same bits and the same status —
# and both must run the resumed body over the value they were handed.

(let [s :user_sig_893
      f (fiber/new (fn []
                     (let [r (emit s {:p 1})]
                       [:resumed r])) |:user_sig_893|)]
  (assert (= (fiber/resume f) {:p 1}) "dynamic emit delivers its payload")
  (assert (= (fiber/status f) :paused) "dynamic emit suspends the fiber")
  (assert (= (fiber/bits f) user-bits)
          "dynamic emit reports the same bit as the literal emit")
  (assert (= (fiber/resume f "MEDIATED") [:resumed "MEDIATED"])
          "the resume value is the result of the dynamic emit"))

# ── A compound literal emit keeps both halves ────────────────────────
#
# `:yield` survives any truncation on its own, so a compound signal
# suspends either way. The user bit is what a mediator needs to tell
# `|:yield :user_sig_893|` from a bare `(yield)`.

(let [f (fiber/new (fn [] (emit |:yield :user_sig_893| :data))
                   |:yield :user_sig_893|)]
  (assert (= (fiber/resume f) :data)
          "compound literal emit delivers its payload")
  (assert (= (bit/and (fiber/bits f) 2) 2) "compound literal emit keeps :yield")
  (assert (= (bit/and (fiber/bits f) user-bits) user-bits)
          "compound literal emit keeps the user signal's bit"))

# ── A mask that does not name the signal does not catch it ───────────
#
# Routing reads the emitted bits. With the bits truncated away there is
# nothing left to decide against, and the payload reaches a parent whose
# mask never named the signal.

(signal :user_sig_893b)
(def user-bits-b (bit/shift-left 1 (get (signals) :user_sig_893b)))
(let [inner (fiber/new (fn []
                         (let [r (emit :user_sig_893b :escaped)]
                           [:resumed r])) |:yield|)
      outer (fiber/new (fn [] (fiber/resume inner)) |:user_sig_893b|)]
  (assert (= (fiber/resume outer) :escaped)
          "a mask that does not name the signal propagates it to the parent")
  (assert (= (fiber/status inner) :paused)
          "the fiber whose signal propagated is suspended, not finished")
  (assert (= (fiber/bits inner) user-bits-b)
          "the propagated signal keeps the user signal's bit"))

# ── A squelch boundary naming the signal holds ───────────────────────
#
# `squelch` enforces against the bits the closure actually produces. A
# literal emit of a squelched signal must be converted into a
# `signal-violation`, exactly as the dynamic emit of the same signal is.

(signal :user_sig_893c)
(let [g (squelch (fn [] (emit :user_sig_893c {:p 1})) |:user_sig_893c|)
      [ok? _] (protect (g))]
  (assert (not ok?) "a squelch boundary catches a literal emit of the signal"))

(let [s :user_sig_893c
      h (squelch (fn [] (emit s {:p 2})) |:user_sig_893c|)
      [ok? _] (protect (h))]
  (assert (not ok?) "a squelch boundary catches a dynamic emit of the signal"))
