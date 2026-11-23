(elle/epoch 18)
# audited: 2026-11-22
# Every spec document imports, every fence's claims hold, and every fence's source compiles.
# docs/spec.md
#
# The counter-factual: `make doctest` only builds each document's module, so a
# fence whose claim went false, or whose source does not compile, would pass
# it. This file stops at the first such fence.

# A document imports each document whose functions it calls, and the default
# loader runs a module again at every import, so the tree would build its
# documents thousands of times. This loader loads each path once, and its
# answer calls the module's function once. A call with arguments, as std/proof
# makes of its parts, is a parametric module's, and runs each time.
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

(defn document [file]
  "The struct the spec document FILE answers, or nil for one that holds no fence."
  (def [ok? m]
    (once (fn [] (protect (import (string "../../docs/spec/" file))))))
  (cond
    ok? (once m)
    (string/contains? (get m :message) "holds 0 forms") nil
    (error m)))

(def dir (path/join (path/parent (get (meta/location) :file)) "../../docs/spec"))

(each file in (sort (file/ls dir))
  (when (and (= "md" (path/extension file)) (not= file "AGENTS.md"))
    (let [doc (document file)]
      (unless (nil? doc)
        (each fence in (get doc :fences)
          (fence:check)
          (when (string? fence:source)
            (compile/lir fence:source (string file ":" fence:name))))))))

(println "spec: ok")
