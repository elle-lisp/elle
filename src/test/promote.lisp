(elle/epoch 18)
# audited: 2026-11-24
## elle test — --promote: render an ad-hoc form's syntax into <corpus>/<name>.lisp.
## docs/test-cli.md
##
## A fragment of one module (see store.lisp).

(defn do-promote [conn opts]
  (let [id (get (get opts :promote) 0)
        name (get (get opts :promote) 1)
        rows (sqlite:query conn
                           "SELECT src AS src FROM form WHERE hash = ?1 LIMIT 1"
                           [id])]
    (if (empty? rows)
      (begin
        (eprintln "promote: no form with id " id)
        (os/exit 1))
      (let [src (get (get rows 0) :src)
            dir (get opts :corpus)
            out (string dir "/" name ".lisp")]
        (file/mkdir-all dir)
        (spit out (string "(elle/epoch 10)\n" src "\n"))
        (sqlite:close conn)
        (os/exit 0)))))
