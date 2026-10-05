(elle/epoch 14)
# audited: 2026-10-05
## elle test — --repin: after the run, move each ledger to what the run read,
## adopt what it had no row for, and refuse what regressed.
## docs/ratchet.md
##
## A fragment of one module (see store.lisp).

(def repin ((import "std/ratchet/repin")))

# --repin moves the rows of the run's build, so a run with no build has none
# to move. Refuse it before anything runs, rather than run the selection and
# leave the ledger as it was (docs/test-cli.md).
(defn refuse-repin-without-build []
  (eprintln "elle test: no build, so --repin has nothing to move: run it under elle-rig test")
  (os/exit 2))

(defn worst-of [readings better]
  "The reading a pin moves to when several tiers read one subject: the one
   on the worse side, so the new pin holds on every tier."
  (let [@w nil]
    (each r in readings
      (when (or (nil? w)
                (if (= better :higher)
                  (< (get r :value) (get w :value))
                  (> (get r :value) (get w :value))))
        (assign w r)))
    w))

(defn grouped [readings key-of]
  "READINGS in groups keyed by KEY-OF, in first-seen order: [[key rs] ...]."
  (let [@order @[]
        @groups @{}]
    (each r in readings
      (let [k (key-of r)]
        (when (nil? (get groups k))
          (push order k)
          (put groups k @[]))
        (push (get groups k) r)))
    (map (fn [k] [k (get groups k)]) order)))

(defn any-verdict? [readings verdict]
  (not (empty? (filter (fn [r] (= (get r :verdict) verdict)) readings))))

(defn say [verb file r what]
  (eprintln "  " verb "  " file "  " (get r :subject) "  "
            (string (get r :axis)) "  " what))

# One subject on one axis, as every tier read it. A regression anywhere is
# refused: a bound moves the worse way by hand, where the diff shows it beside
# the change that needed it (docs/ratchet.md § The runner). A subject with no
# row is adopted; a stale one moves to the worst of its readings.
(defn repin-group [file text row rs]
  "The ledger TEXT after this group's move, or TEXT itself when nothing moved."
  (let [r0 (first rs)]
    (cond
      (any-verdict? rs :regression)
        (begin
          (say "refuse" file r0
               (string "a regression against "
                       (ledger:describe-bound (get row :kind) (get row :bound))
                       " moves the worse way by hand"))
          text)
      (nil? row)
        (let [row-text (repin:row-for r0 adopting-build)]
          (say "adopt" file r0 row-text)
          (repin:adopt text row-text))
      (any-verdict? rs :stale)
        (let [w (worst-of rs (get row :better))
              token (repin:write-number (get w :value) (get w :half))
              moved (repin:move text (get r0 :subject) (get r0 :axis) token
                                (get row :build))]
          (if moved
            (begin
              (say "repin" file r0
                   (string (ledger:describe-bound (get row :kind)
                           (get row :bound)) " → " token))
              moved)
            (begin
              (say "refuse" file r0 "no row found in the ledger's text")
              text)))
      true text)))

(defn repin-file [file queued]
  "Move FILE's ledger to what QUEUED read, and write it back when it moved."
  (let [l (get ledgers (ledger:producer-of file))
        rows (get l :rows)
        before (slurp (get l :file))
        @text before]
    (each entry in (grouped queued
                            (fn [r]
                              (ledger:row-key (get r :subject) (get r :axis))))
      (assign
        text
        (repin-group file text (get rows (get entry 0)) (get entry 1))))
    (when (not (= text before)) (spit (get l :file) text))
    nil))

(defn repin-ledgers []
  "Move every ledger the run judged against, one file at a time."
  (each entry in (grouped repin-queue (fn [r] (get r :file)))
    (repin-file (get entry 0) (get entry 1)))
  nil)
