#!/usr/bin/env elle
(for-each (fn (name)
  (if (string/contains? (string name) "sub")
      (display (-> (string name) (append "\n")))
      nil))
  (help))
