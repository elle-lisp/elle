(elle/epoch 6)
#!/usr/bin/env elle
## VM bug repro: "Expected cell, got closure"
##
## Crashes with the extra (def glob-plugin ...) binding.
## Does NOT crash when inlining the import.

(def glob-plugin (import "target/release/libelle_glob.so"))
(def do-glob (get glob-plugin :glob))
(def out @[])

(each file in (do-glob "*.lisp")
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? forms] (protect (read-all src))]]
        (when ok?
          (each form in forms
            (when (pair? form)
              (push out (string (first form))))))))))

(println (length out))
