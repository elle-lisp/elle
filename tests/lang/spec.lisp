(elle/epoch 18)
# audited: 2026-11-22
# Each spec document with code is one module of its rule functions and its fences, and every fence's claims hold.
# docs/spec.md
#
# The counter-factual: `make doctest` only builds each document's module, so a
# fence whose claim went false would pass it. Only the :check this file runs
# fails on that fence. A document that kept two fences fails its import here.

(def documents
  ["aliased" "anew" "borrows" "call" "capture" "closure" "crossing" "datalog"
   "form" "free" "group" "handover" "itself" "liveness" "mutual" "owner"
   "payload" "place" "recur" "recursion" "release" "removal" "replace" "return"
   "signature" "sink" "states" "steps" "stored" "through"])

(def no-code ["ir" "judgment" "memory" "ownership"])

# A document imports each document whose functions it calls, and the default
# loader keeps no cache, so every import path would load its target again:
# thousands of loads for the whole tree. This loader loads each path once, and
# its answer calls the module's function once. A call with arguments, as
# std/proof makes of its parts, is a parametric module's, and runs each time.
(def loaded @{})

(defn load-once [request]
  "The module REQUEST names, loaded once per path. A module that is a function
   answers as one that calls it the first time with no arguments and answers
   that value after, and calls it anew with arguments."
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

(defn load [name]
  "Import the spec document NAME, a module, and answer [ok? value]."
  (once (fn [] (protect (import (string "../../docs/spec/" name ".md"))))))

(def dir (path/join (path/parent (get (meta/location) :file)) "../../docs/spec"))
(assert (= (set ;(file/ls dir))
           (set "AGENTS.md"
                ;(map (fn [n] (string n ".md")) (concat documents no-code))))
        "the two lists name every document under spec")

(each name in no-code
  (let [[ok? err] (load name)]
    (assert (and (not ok?) (= :module-error (get err :error))
                 (string/contains? (get err :message) "holds 0 forms"))
            (string name ".md holds no fence"))))

(def fence-keys |:name :doc :source :facts :check|)

(defn fence-shape [name fence]
  "The first way FENCE of the document NAME breaks the fence table, or nil."
  (cond
    (not (struct? fence)) (string name ": a fence is a struct")
    (not= fence-keys (set ;(keys fence)))
      (string name ": a fence holds exactly " fence-keys)
    (not (symbol? fence:name)) (string name ": a fence's :name is a symbol")
    (not (string? fence:doc))
      (string name " " fence:name ": its :doc is a string")
    (not (or (nil? fence:source) (string? fence:source)))
      (string name " " fence:name ": its :source is a string or nil")
    (not (struct? fence:facts))
      (string name " " fence:name ": its :facts is a struct")
    (not (and (closure? fence:check) (= 0 (fn/arity fence:check))))
      (string name " " fence:name ": its :check is a function of no arguments")
    nil))

(def failures @[])

(each name in documents
  (let [[ok? m] (load name)]
    (unless ok?
      (assert false (string name ".md imports as a module: " (get m :message))))
    (let [doc (once m)]
      (assert (and (struct? doc) (array? doc:fences))
              (string name ".md answers a struct with its :fences"))
      (each k in (keys doc)
        (unless (= k :fences)
          (assert (closure? (get doc k))
                  (string name ".md answers " k ", a rule function"))))
      (let [names (map (fn [f] (get f :name)) doc:fences)]
        (assert (= (length names) (length (set ;names)))
                (string name ".md names each fence once")))
      (each fence in doc:fences
        (let [bad (fence-shape name fence)]
          (assert (nil? bad) bad)
          (let [[ran? err] (protect (fence:check))]
            (unless ran?
              (push failures
                    (string name " " fence:name ": " (get err :message))))))))))

(assert (empty? failures) (string "every fence's claims hold: " failures))
(println "spec: ok")
