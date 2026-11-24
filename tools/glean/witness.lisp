(elle/epoch 18)
# audited: 2026-11-24
# Witnesses: a fence's function run in a child, to confirm a leak, a use after free or a hold that grows.
# tools/glean/witness.md

(fn []
  # A leak or a hold keeps whole objects, and a shape that keeps nothing reads
  # a tenth of one or less.
  (def threshold 0.5)

  (defn prelude [entry]
    "The fence's source, then its function bound to a mutable name, and a
  thunk per argument list that builds the list anew."
    (string (get entry :source) "\n(def @f " (get entry :name) ")"
            "\n(def lists ["
            (string/join (map (fn [a] (string "(fn [] " a ")"))
                              (get entry :args)) " ") "])\n"))

  (def drivers
    {:none "(def r ((import \"std/ratchet\")))
(each a in lists
  (let [g (first (r:rate \"witness\" (fn [j] (f ;(a))) :on [r:objects]
                         :block 50 :min 4 :max 20))]
    (println \"witness \" (- (get g :value) (get g :half)))))\n"
     :early "(each a in lists
  (each _ in (range 100) (f ;(a))))\n"
     :late "(def r ((import \"std/ratchet\")))
(def g (r:crest \"witness\" (fn [n] (f ;(sized n))) :n 50))
(println \"witness \" (get g :value))\n"})

  (defn run [elle program flags]
    "{:exit :stdout :stderr} of the child that runs PROGRAM under FLAGS."
    (with-temp-dir dir
      (let [path (path/join dir "witness.lisp")]
        (file/write path program)
        (subprocess/system elle [;flags path]))))

  (defn measures [out]
    "The numbers the child's witness lines print in OUT."
    (->array (map (fn [line] (parse-float (slice line 8)))
                  (filter (fn [line] (string/starts-with? line "witness "))
                          (string/split out "\n")))))

  (defn verdict [elle entry reading]
    "The verdict on READING for the function ENTRY names."
    (def sized (get entry :sized))
    (if (and (= reading :late) (nil? sized))
      :unsized
      (let [program (string (prelude entry)
                            (if (nil? sized)
                              ""
                              (string "(def sized " sized ")\n"))
                            (get drivers reading))
            early? (= reading :early)
            got (run elle program (if early? ["--trace=guardfree"] []))]
        (cond
          (and early? (string/contains? (get got :stderr) "[guardfree] SIGSEGV"))
            :witnessed
          (not= 0 (get got :exit)) :error
          early? :unwitnessed
          (any? (fn [m] (>= m threshold)) (measures (get got :stdout)))
            :witnessed
          :unwitnessed))))

  (defn witness [elle entry readings]
    "The verdict on each of READINGS for the function ENTRY names, run by the
  binary ELLE."
    (from-pairs (map (fn [reading] [reading (verdict elle entry reading)])
                     readings)))

  {:witness witness})
