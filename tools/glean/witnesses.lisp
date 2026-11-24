(elle/epoch 18)
# audited: 2026-11-24
# The witnesses: each fence the table names, its diff readings, and the verdict of a run on each.
# tools/glean/witness.md

# Each spec document imports the documents whose functions it calls, so this
# loader loads each path once, as the census's does.
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

(def {:diff diff} (once (fn [] ((import "./diff")))))
(def {:witness witness} (once (fn [] ((import "./witness")))))
(def table (once (fn [] ((import "./args")))))
(def elle (elle/executable))

(def entries
  (from-pairs (map (fn [e] [(string (get e :document) ":" (get e :name)) e])
                   table)))

(defn document [file]
  "The struct the spec document FILE answers, or nil for one that holds no fence."
  (def [ok? m]
    (once (fn [] (protect (import (string "../../docs/spec/" file))))))
  (cond
    ok? (once m)
    (string/contains? (get m :message) "holds 0 forms") nil
    (error m)))

(defn readings [answer]
  "The count of each reading the diff's ANSWER gives outside the entry function."
  (def called (filter (fn [f] (not (nil? (get f :id)))) (get answer :functions)))
  (from-pairs (filter (fn [[k n]] (> n 0))
                      (map (fn [k]
                             [k (sum (map (fn [f] (length (get f k))) called))])
                           [:none :early :late]))))

(def spec-dir
  (path/join (path/parent (get (meta/location) :file)) "../../docs/spec"))
(def unnamed @[])
(each file in (sort (filter (fn [f] (= "md" (path/extension f)))
                            (file/ls spec-dir)))
  (let [doc (document file)]
    (unless (nil? doc)
      (each fence in (get doc :fences)
        (let [source (get fence :source)
              key (string file ":" (get fence :name))]
          (when (string? source)
            (let [found (readings (diff (compile/lir source key)))]
              (cond
                (empty? found) nil
                (has? entries key)
                  (println key " " found " "
                           (witness elle (put (get entries key) :source source)
                                    (keys found)))
                (push unnamed key)))))))))

(println (length unnamed) " fences with a reading the table does not name:")
(each key in unnamed
  (println key))
