(elle/epoch 14)
# audited: 2026-10-11
# A container that loses its reference to itself still holds its own region until its last release.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, so a read of a freed page faults.
#
# Each shape stores a container into itself, removes that reference, drops
# the removed value, and then reads the container again.
#
# The counter-factual: a funnel that skips the self-reference on the store
# but not on the removal lowers a count no store raised. The container's
# region then reaches zero when the removed value is released, and the read
# after it faults. region-self-store.lisp holds the store half.

(defn pop-then-read []
  "Pop an @array's reference to itself, then read the @array."
  (let [a @[]]
    (push a a)
    (pop a)
    (push a 1)
    (length a)))

(defn remove-then-read []
  "Remove an @array's reference to itself by index, then read the @array."
  (let [a @[0]]
    (push a a)
    (remove a 1)
    (length a)))

(defn overwrite-then-read []
  "Overwrite an @array's reference to itself, then read the @array."
  (let [a @[0]]
    (put a 0 a)
    (put a 0 7)
    (get a 0)))

(defn del-then-read []
  "Delete an @struct's reference to itself, then read the @struct."
  (let [s @{:k 1}]
    (put s :me s)
    (del s :me)
    (get s :k)))

(defn rebox-then-read []
  "Overwrite a box's reference to itself, then read the box."
  (let [b (box nil)]
    (rebox b b)
    (rebox b 3)
    (unbox b)))

(repeat 50
        (assert (= (pop-then-read) 1) "the @array survives the pop of itself")
        (assert (= (remove-then-read) 1)
                "the @array survives the removal of itself")
        (assert (= (overwrite-then-read) 7)
                "the @array survives the overwrite of itself")
        (assert (= (del-then-read) 1)
                "the @struct survives the deletion of itself")
        (assert (= (rebox-then-read) 3)
                "the box survives the overwrite of itself"))

(println "region-self-store-uaf: OK")
