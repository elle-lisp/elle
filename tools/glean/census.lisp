(elle/epoch 18)
# audited: 2026-11-22
# The census: each spec fence with a source, compiled and gleaned, and its facts judged beside the hand facts.
# tools/glean/overview.md

# A document imports each document whose functions it calls, and the default
# loader keeps no cache, so every import path would load its target again.
# This loader loads each path once, and its answer calls the module's function
# once; a call with arguments, a parametric module's, runs each time.
(def loaded @{})

(defn load-once [request]
  "The module REQUEST names, loaded once per path."
  (let [path (import/resolve (get request :spec) (get request :dir))]
    (unless (has? loaded path)
      (let [f (import/load request)
            @called false
            @value nil]
        (put loaded path
             (if (closure? f)
               (fn [& args]
                 (cond
                   (nonempty? args) (f ;args)
                   called value
                   true
                     (begin
                       (assign value (f))
                       (assign called true)
                       value)))
               f))))
    (get loaded path)))

(defn once [thunk]
  "Run THUNK with each import it makes answered by load-once."
  (parameterize ((*loader* load-once))
    (thunk)))

(def {:row row} (once (fn [] ((import "./rows")))))
(def {:glean glean} (once (fn [] ((import "./glean")))))

(defn document [file]
  "The struct the spec document FILE answers, or nil for one that holds no fence."
  (def [ok? m]
    (once (fn [] (protect (import (string "../../docs/spec/" file))))))
  (cond
    ok? (once m)
    (string/contains? (get m :message) "holds 0 forms") nil
    (error m)))

(defn stopped [file fence err]
  "The row of FENCE, of FILE, that the error ERR stopped."
  {:document file :name (get fence :name) :error (get err :message)})

(defn fence-row [file fence]
  "The census row of FENCE, of the document FILE, whose :source is a string. The
  compile's name is FILE:NAME, because a name that ends in .md reads as markdown."
  (def where (string file ":" (get fence :name)))
  (def [compiled? unit] (protect (compile/lir (get fence :source) where)))
  (if (not compiled?)
    (stopped file fence unit)
    (let [[gleaned? answer] (protect (glean unit))]
      (if gleaned? (row file fence answer) (stopped file fence answer)))))

(def spec-dir
  (path/join (path/parent (get (meta/location) :file)) "../../docs/spec"))
(def rows @[])
(each file in (sort (filter (fn [f] (= "md" (path/extension f)))
                            (file/ls spec-dir)))
  (let [doc (document file)]
    (unless (nil? doc)
      (each fence in (get doc :fences)
        (when (string? (get fence :source)) (push rows (fence-row file fence)))))))

(defn unmatched [r]
  "How many names row R lists in one answer alone."
  (if (has? r :error)
    0
    (+ (length (get (get r :released) :hand))
       (length (get (get r :released) :glean))
       (length (get (get r :refused) :hand))
       (length (get (get r :refused) :glean)))))

(each r in rows
  (println r))
(println (length rows) " fences, "
         (length (filter (fn [r] (has? r :error)) rows))
         " stopped by an error, "
         (length (filter (fn [r] (> (unmatched r) 0)) rows))
         " with a mismatch, " (sum (map unmatched rows))
         " names in one answer alone")
