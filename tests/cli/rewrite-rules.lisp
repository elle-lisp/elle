(elle/epoch 18)
## audited: 2026-11-22
## `elle rewrite --list-rules` prints every migration rule under the epoch that adds it and that epoch's summary.
## tests/cli/overview.md
## docs/epochs.md

(include-file "../runner/lib/harness.lisp")

(def out
  (let [r (elle-run ["rewrite" "--list-rules"])]
    (assert (= r:exit 0) (string "--list-rules failed: " r:err))
    r:out))

(defn at [needle]
  "Where NEEDLE starts in the listing."
  (let [i (string/find out needle)]
    (assert (not (nil? i)) (string "--list-rules must print " needle ": " out))
    i))

(defn between [needle after before]
  "Require NEEDLE to stand after AFTER and before BEFORE."
  (let [i (at needle)]
    (assert (< (at after) i) (string needle " stands before " after))
    (when before
      (assert (< i (at before)) (string needle " stands after " before)))))

(eprintln "scenario: every epoch heads its rules, in order")
(let [rest (slice out (+ (at "current epoch: ") (length "current epoch: ")))
      current (parse-int (slice rest 0 (string/find rest ")")))]
  (def @last 0)
  (each epoch (range 1 (+ current 1))
    (let [i (at (string "\nepoch " epoch ": "))]
      (assert (> i last) (string "epoch " epoch " comes after the one before"))
      (assign last i))))

# A flat list hid which epoch a rule belongs to, so the reader of a file that
# declares epoch 15 could not tell which rules reach it.
(eprintln "scenario: a rule stands under the epoch that adds it")
(between "replace: import-file (arity 1) → (import $1)"
         "\nepoch 16: import-file goes" "\nepoch 17: ")
(between "lexical: string-escapes" "\nepoch 13: " "\nepoch 14: ")

# Epoch 3 renames display to print, and epoch 2 renamed print to println. Each
# epoch's own rule is what a file of that epoch meets.
(eprintln "scenario: a rename is listed as its epoch wrote it")
(between "rename: display → print\n" "\nepoch 3: " "\nepoch 4: ")

# Epoch 14 rewrites a millisecond bound as `:timeout` in seconds. A rule the
# listing leaves out is a rewrite nobody reading it knows about.
(eprintln "scenario: the calls whose durations moved to seconds are named")
(each call ["sys/join" "chan/select" "io/wait" "ev/step" "udp/recv-from"]
  (at call))

(eprintln "scenario: the defs-before-reads rule stands under epoch 18")
(between "defs-before-reads: " "\nepoch 18: " nil)

# A reader of an epoch 15 module learns here that its forms fold into its last
# function.
(eprintln "scenario: the module fold stands under epoch 16")
(between "fold-module: " "\nepoch 16: " "\nepoch 17: ")

(eprintln "all rewrite-rules scenarios passed")
