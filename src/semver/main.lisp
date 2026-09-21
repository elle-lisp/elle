(elle/epoch 12)
# audited: 2026-09-21
## elle semver — argv, dispatch, report rendering, and the exit contract.
## docs/semver.md

(def sfile ((import "std/semver/file")))
(def surf ((import "std/semver/surface")))
(def sdiff ((import "std/semver/diff")))
(def arb ((import "std/semver/arbitrate")))
(def sv ((import "std/semver")))

# ── exits and small helpers ──────────────────────────────────────────
# Exit 2 is the tool failing; exit 1 is the tool succeeding at saying no.
(defn die [msg]
  (eprintln (string "elle semver: " msg))
  (os/exit 2))

(defn refuse [msg]
  (eprintln (string "elle semver: " msg))
  (os/exit 1))

(defn err-text [e]
  (or (and (struct? e) (get e :message)) (string e)))

(defn has? [xs x]
  (any? (fn [y] (= y x)) (->list xs)))

(defn pad [s n]
  (let [@out (string s)]
    (while (< (length out) n) (assign out (string out " ")))
    out))

(defn pad2 [n]
  (if (< n 10) (string "0" n) (string n)))

(defn today-utc []
  "The UTC calendar date, YYYY-MM-DD, from the wall clock."
  (let [days (floor (/ (clock/realtime) 86400))
        z (+ days 719468)
        era (floor (/ z 146097))
        doe (- z (* era 146097))
        a (+ (- doe (floor (/ doe 1460)))
             (- (floor (/ doe 36524)) (floor (/ doe 146096))))
        yoe (floor (/ a 365))
        doy (- doe (+ (* 365 yoe) (- (floor (/ yoe 4)) (floor (/ yoe 100)))))
        mp (floor (/ (+ (* 5 doy) 2) 153))
        d (+ (- doy (floor (/ (+ (* 153 mp) 2) 5))) 1)
        m (if (< mp 10) (+ mp 3) (- mp 9))
        y (+ yoe (* era 400) (if (<= m 2) 1 0))]
    (string y "-" (pad2 m) "-" (pad2 d))))

# ── argv ─────────────────────────────────────────────────────────────
(defn drop-sep [args]
  (if (and (not (empty? args)) (= (first args) "--")) (rest args) args))

(def flag-spec
  @{"--json" [:json :flag]
    "--tests" [:tests :value]
    "--tag" [:tag :flag]
    "--no-tests" [:no-tests :flag]
    "--strict" [:strict :flag]})

(defn parse-args [args acc]
  (if (empty? args)
    acc
    (let [a (first args)
          spec (get flag-spec a)]
      (if (= spec nil)
        (begin
          (put acc :paths (concat (get acc :paths) [a]))
          (parse-args (rest args) acc))
        (let [flag? (= (get spec 1) :flag)]
          (put acc (get spec 0) (if flag? true (first (rest args))))
          (parse-args (if flag? (rest args) (rest (rest args))) acc))))))

# ── targets ──────────────────────────────────────────────────────────
(defn module-of [path]
  "The import spec a module path answers to: lib/X.lisp is std/X."
  (let [p (path/normalize path)]
    (if (string/starts-with? p "lib/")
      (string "std/" (slice p 4 (- (length p) 5)))
      (string "std/" (path/stem p)))))

(defn resolve-target [arg]
  (cond
    (file/exists? arg) {:path arg :module (module-of arg)}
    (string/starts-with? arg "std/")
      (let [p (string "lib/" (slice arg 4 (length arg)) ".lisp")]
        (if (file/exists? p)
          {:path p :module arg}
          (die (string arg ": no module at " p))))
    (die (string arg ": not a module file or import spec"))))

(defn extract-target [t]
  (let [[ok? s] (protect (surf:extract {:path (t :path) :module (t :module)}))]
    (unless ok?
      (die (string (t :path) ": " (err-text s))))
    s))

(defn claimed-version [t s]
  (or (get s :version)
      (die (string (t :path) " declares no (elle/version \"X.Y.Z\")"))))

(defn surface-path [path]
  (path/with-extension path "surface"))

(defn read-baseline [spath]
  (let [[ok? b] (protect (sfile:parse (file/read spath)))]
    (unless ok?
      (die (string spath ": " (err-text b))))
    b))

# ── rendering ────────────────────────────────────────────────────────
(defn kws [ks]
  (string "["
          (string/join (map (fn [k] (string ":" (string k))) (->list ks)) " ")
          "]"))

(defn brief [x]
  "One side of a change, compressed to a report phrase."
  (cond
    (nil? x) "-"
    (and (struct? x) (= (get x :kind) :fn))
      (string "fn " (sfile:render-shape x) " :signals "
              (kws ((x :signals) :bits)))
    (and (struct? x) (= (get x :kind) :value))
      (string ":type :" (string (x :type)) " :hash \"" (x :hash) "\"")
    (and (struct? x) (not (nil? (get x :required)))) (sfile:render-shape x)
    (string x)))

(defn change-line [c]
  (string "  " (pad (string (c :change)) 26) " " (pad (string (c :export)) 14)
          " " (brief (c :was)) " -> " (brief (c :now))))

(defn verdict-text [v]
  (if (= (v :verdict) :sufficient)
    "sufficient"
    (string "INSUFFICIENT (need >= " (v :required) ")")))

(defn print-report [module bv claimed d v]
  (println (string module "  " bv " (.surface) -> worktree"))
  (println "")
  (each fl [:major :minor :patch :none]
    (let [cs (->list (filter (fn [c] (= (c :floor) fl)) (->list (d :changes))))]
      (unless (empty? cs)
        (println (string fl))
        (each c cs
          (println (change-line c))))))
  (println "")
  (println (string "floor: " (string (d :floor)) "    claimed: " claimed
                   "    verdict: " (verdict-text v))))

(defn print-json [module bv claimed d v]
  (println (json/serialize {:module module
                            :baseline bv
                            :claimed claimed
                            :floor (d :floor)
                            :required (v :required)
                            :verdict (v :verdict)
                            :changes (d :changes)})))

# ── the dev loop ─────────────────────────────────────────────────────
(defn judge [base s claimed]
  "Baseline x worktree: {:bv :d :v :code}."
  (let [bv (base :version)
        d (sdiff:diff base s)
        v (sdiff:verdict {:baseline bv :claimed claimed :floor (d :floor)})]
    {:bv bv :d d :v v :code (if (= (v :verdict) :sufficient) 0 1)}))

(defn unchanged? [j claimed]
  (and (empty? (->list ((j :d) :changes))) (= claimed (j :bv))))

(defn print-initial [t]
  (println (string (t :module) ": no .surface baseline (initial); elle semver"
                   " release creates it")))

(defn print-status [t j claimed]
  (if (unchanged? j claimed)
    (println (string (t :module) " " (j :bv) ": surface unchanged (floor: none)"))
    (print-report (t :module) (j :bv) claimed (j :d) (j :v))))

(defn status-target [t json?]
  (let [s (extract-target t)
        claimed (claimed-version t s)
        spath (surface-path (t :path))]
    (if (not (file/exists? spath))
      (begin
        (print-initial t)
        0)
      (let [j (judge (read-baseline spath) s claimed)]
        (if json?
          (print-json (t :module) (j :bv) claimed (j :d) (j :v))
          (print-status t j claimed))
        (j :code)))))

(defn status-line [t]
  "One dashboard line; answers the exit code the module earns."
  (let [s (extract-target t)
        claimed (claimed-version t s)
        spath (surface-path (t :path))]
    (if (not (file/exists? spath))
      (begin
        (println (string (t :module) " " claimed ": initial (no .surface)"))
        0)
      (let [j (judge (read-baseline spath) s claimed)]
        (if (unchanged? j claimed)
          (println (string (t :module) " " (j :bv)
                           ": surface unchanged (floor: none)"))
          (println (string (t :module) " " (j :bv) " -> " claimed ": floor "
                           (string ((j :d) :floor)) ", verdict "
                           (verdict-text (j :v)))))
        (j :code)))))

# ── check: verdict + old-test arbitration ────────────────────────────
(defn major-claim? [bv claimed]
  "Does CLAIMED bump the major position over BV (pre-1.0: the y)?"
  (>= (sv:compare claimed (sdiff:required bv :major)) 0))

(defn run-arbitration [t base opts]
  (cond
    (get opts :no-tests) {:status :skipped :reason "--no-tests"}
    (arb:arbitrate {:base base
                    :module (t :module)
                    :exe (elle/executable)
                    :worktree "."})))

(defn print-arbitration [res strict?]
  (match (res :status)
    :skipped
      (println (string "arbitration: skipped (" (res :reason) ")"))
    :pass (println "arbitration: previous release's tests pass against the worktree")
    :fail (println "compat claim rejected: previous release's tests fail against the worktree")
    :unavailable
      (println (string (if strict? "" "note: ") "arbitration unavailable: "
                       (res :reason)))
    _ nil))

(defn check-target [t opts]
  (let [s (extract-target t)
        claimed (claimed-version t s)
        spath (surface-path (t :path))
        json? (get opts :json)
        strict? (get opts :strict)]
    (if (not (file/exists? spath))
      (begin
        (if json?
          (println (json/serialize {:module (t :module)
                                    :claimed claimed
                                    :initial true}))
          (print-initial t))
        0)
      (let [base (read-baseline spath)
            j (judge base s claimed)
            res (if (major-claim? (j :bv) claimed)
                  {:status :skipped
                   :reason "a major claim promises no compatibility"}
                  (run-arbitration t base opts))
            gates? (or (= (res :status) :fail)
                       (and strict? (= (res :status) :unavailable)))
            code (if (and gates? (< (j :code) 1)) 1 (j :code))]
        (if json?
          (println (json/serialize {:module (t :module)
                                    :baseline (j :bv)
                                    :claimed claimed
                                    :floor ((j :d) :floor)
                                    :required ((j :v) :required)
                                    :verdict ((j :v) :verdict)
                                    :changes ((j :d) :changes)
                                    :arbitration {:status (res :status)
                                    :reason (get res :reason)}}))
          (begin
            (print-status t j claimed)
            (print-arbitration res strict?)))
        code))))

# ── the walk ─────────────────────────────────────────────────────────
(defn module-version [path]
  "The declared version of the file at PATH, or nil."
  (let [[ok? text] (protect (file/read path))]
    (if (and ok? (string/contains? text "(elle/version \""))
      (let [[ok2? v] (protect (surf:declared-version text))]
        (if ok2? v nil))
      nil)))

(defn walk-tree [dir out]
  (each name (sort (->list (file/ls dir)))
    (let [p (if (= dir ".") name (path/join dir name))]
      (cond
        (has? [".git" ".jj" "target" "node_modules"] name) nil
        (file/directory? p) (walk-tree p out)
        (and (string/ends-with? name ".lisp") (module-version p)) (push out p)
        nil)))
  out)

(defn dashboard [json?]
  (let [found (->list (walk-tree "." @[]))]
    (cond
      (empty? found) (begin
                       (println "no versioned modules found")
                       0)
      (empty? (rest found))
        (status-target (resolve-target (first found)) json?)
      (let [@worst 0]
        (each p found
          (let [c (status-line (resolve-target p))]
            (when (> c worst) (assign worst c))))
        worst))))

# ── release ──────────────────────────────────────────────────────────
(defn head-commit []
  (let [[ok? oid] (protect (let [git ((import "std/git"))
                                 repo (git:open ".")
                                 oid (git:resolve repo "HEAD")]
                             (git:close repo)
                             oid))]
    (if ok? oid nil)))

(defn create-tag [name]
  (let [[ok? e] (protect (let [git ((import "std/git"))
                               repo (git:open ".")]
                           (git:tag-create repo name "HEAD")
                           (git:close repo)))]
    (unless ok?
      (die (string "cannot tag " name ": " (err-text e))))))

(defn guard-release [t s claimed spath]
  "Refuse a dishonest claim before anything is written."
  (when (file/exists? spath)
    (let [j (judge (read-baseline spath) s claimed)
          cmpv (sv:compare claimed (j :bv))]
      (when (= ((j :v) :verdict) :insufficient)
        (refuse (string (t :module) " " claimed " claims below the floor "
                        (string ((j :d) :floor)) "; need >= " ((j :v) :required))))
      (when (< cmpv 0)
        (refuse (string (t :module) " " claimed " walks the version back from "
                        (j :bv))))
      (when (and (= cmpv 0) (not (= ((j :d) :floor) :none)))
        (refuse (string (t :module) " " claimed " does not move past " (j :bv)
                        " over a " (string ((j :d) :floor)) " change"))))))

(defn release-target [t opts]
  (let [s (extract-target t)
        claimed (claimed-version t s)
        spath (surface-path (t :path))]
    (guard-release t s claimed spath)
    (let [glob (or (get opts :tests)
                   (string "tests/elle/" (path/filename (t :module)) "*.lisp"))
          commit (head-commit)
          @rec (merge s {:tests glob})]
      (when commit
        (assign rec (put rec :released {:commit commit :date (today-utc)})))
      (file/write spath (sfile:render rec))
      (when (get opts :tag)
        (create-tag (string (path/filename (t :module)) "/v" claimed)))
      (println (string "wrote " spath " (" (t :module) " " claimed ")"))
      0)))

# ── main ─────────────────────────────────────────────────────────────
(defn over-paths [paths f]
  "Run F over each resolved path; answer the worst exit code."
  (let [@worst 0]
    (each p paths
      (let [c (f (resolve-target p))]
        (when (> c worst) (assign worst c))))
    worst))

(def args (drop-sep (rest (sys/argv))))
(def cmd
  (cond
    (empty? args) :status
    (= (first args) "release") :release
    (= (first args) "check") :check
    :status))
(def opts (parse-args (if (= cmd :status) args (rest args)) @{:paths []}))
(def paths (get opts :paths))

(def code
  (cond
    (= cmd :release)
      (if (empty? paths)
        (die "release needs a module path")
        (release-target (resolve-target (first paths)) opts))
    (= cmd :check)
      (if (empty? paths)
        (over-paths (->list (walk-tree "." @[])) (fn [t] (check-target t opts)))
        (over-paths paths (fn [t] (check-target t opts))))
    (empty? paths) (dashboard (get opts :json))
    (over-paths paths (fn [t] (status-target t (get opts :json))))))

(os/exit code)
