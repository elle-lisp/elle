(elle/epoch 14)
# audited: 2026-09-30
# when runs its body on a truthy test and unless on a falsy one; each answers nil when its body does not run.
# docs/control.md

(assert (= (when true :yes) :yes) "when runs its body on a truthy test")
(assert (nil? (when false :yes)) "when answers nil on a falsy test")
(assert (= (unless false :yes) :yes) "unless runs its body on a falsy test")
(assert (nil? (unless true :yes)) "unless answers nil on a truthy test")

(assert (= (when true
             1
             2
             3) 3) "when answers its last body form")
(assert (= (unless nil
             1
             2
             3) 3) "unless answers its last body form")
