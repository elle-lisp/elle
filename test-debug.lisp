# Test the lbox fix

(def make-validator
  (fn [check-fn describe-str]
    {:check check-fn :describe describe-str}))

(def compile-validator
  (fn [expr]
    (cond
      ((fn? expr)
       (let [[desc (string expr)]]
         (make-validator
           (fn [value]
             (if (expr value)
               nil
               {:error :validation :expected desc :got (type-of value)}))
           desc)))
      ((struct? expr)
       (let* [[shape-keys (keys expr)]
              [compiled-shape (let [[s @{}]]
                                (each k in shape-keys
                                  (put s k (compile-validator (get expr k))))
                                (freeze s))]
              [desc (let [[parts @[]]]
                      (each k in shape-keys
                         (push parts (append (append (string k) " ")
                                             (get (get compiled-shape k) :describe))))
                       (append (append "{" (string/join parts ", ")) "}"))]]
         (make-validator
           (fn [value]
             (if (not (struct? value))
               {:error :validation
                :expected desc
                :got (type-of value)}
               (let [[failures @[]]]
                 (each k in shape-keys
                   (let* [[sub-v (get compiled-shape k)]
                          [result ((get sub-v :check) (get value k))]]
                     (when (not (nil? result))
                       (push failures {:key k :failure result}))))
                 (if (> (length failures) 0)
                   {:error :validation :fields (freeze failures)}
                   nil))))
           desc)))
      (true
       (error {:error :type-error
               :message "compile-validator: unsupported expression type"})))))

# Warm up with 10 predicate calls
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)
(compile-validator integer?)

# Now try nested
(print "Trying nested...\n")
(def v (compile-validator {:a {:b integer?}}))
(print "Success: " v "\n")
