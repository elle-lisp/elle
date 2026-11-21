(elle/epoch 18)
# audited: 2026-11-20
# A cancelled I/O operation answers once with a `:cancelled` error, and `io/cancel` says whether it reached one.
# docs/impl/io/cancel.md
#
# A reader that keeps its record of a submission learns from that answer that
# the operation has ended. A cancel that reaches nothing returns false, and no
# answer comes for it. Each backend kind a program can make keeps both
# promises.

(defn answers [backend]
  "The next completions the backend answers, or none after 200 waits. One
   wait can return none when all it heard was the cancel's own report, and a
   wait returns at once when nothing is in flight."
  (let [@got []
        @waits 0]
    (while (and (empty? got) (< waits 200))
      (assign waits (+ waits 1))
      (assign got (io/wait backend :timeout 0.05)))
    got))

(defn sleep-request []
  "A fiber parked in a thirty-second sleep, and the request it raised."
  (let [f (fiber/new (fn [] (ev/sleep 30)) 512)]
    (fiber/resume f)
    [f (fiber/value f)]))

(each kind in [:async :mock]
  (let* [backend (io/backend kind)
         [f request] (sleep-request)
         id (io/submit backend request f)]
    (assert (= (io/cancel backend id) true)
            (string kind ": io/cancel reaches the sleep in flight"))
    (let [got (answers backend)]
      (assert (= (length got) 1)
              (string kind ": the cancelled sleep answers once"))
      (let [c (get got 0)]
        (assert (= (get c :id) id) (string kind ": the answer names its id"))
        (assert (= (get (get c :error) :error) :cancelled)
                (string kind ": the answer is a :cancelled error"))))
    (assert (= (io/cancel backend id) false)
            (string kind ": a cancel after the answer reaches nothing"))
    (assert (= (io/cancel backend 999999) false)
            (string kind ": a cancel of an id never minted reaches nothing"))
    (assert (empty? (io/wait backend :timeout 0.05))
            (string kind ": a cancel that reached nothing answers nothing"))
    (assert (= (fiber/status f) :paused)
            (string kind ": the cancel leaves the fiber where it parked"))))
