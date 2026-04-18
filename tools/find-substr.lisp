#!/usr/bin/env elle
(elle/epoch 8)
(for-each (fn (name)
  (if (string/contains? (string name) "sub")
      (display (-> (string name) (append "\n")))
      nil))
  (help))
