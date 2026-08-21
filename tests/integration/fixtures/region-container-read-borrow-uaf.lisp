(elle/epoch 12)
# tests/integration/fixtures/region-container-read-borrow-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression SIGSEGVs under
# --trace=guardfree (and panics on the generation stamp under the plain VM), and
# `make smoke` globs tests/elle/*.lisp into one shared process where a segfault
# would take the whole harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_container_read_borrow_uaf`).
#
# WHAT IT GUARDS — the container-read BORROW: a value read out of a container with
# `get`/`first`/`rest` still lives INSIDE that container, so something must keep it
# alive across the reader. Which something depends on the read form, and the two
# faces fail differently:
#
#   - THE CASCADE FACE (uncounted). `(length (%get c 0))` reads through the OPCODE,
#     which borrows without raising any count. The container's lifetime is the
#     borrow's only protection, so the container is in use for as long as the read's
#     RESULT is (docs/impl/region/rules.md Rule 4, the borrowing node). Anchored at
#     the read, the container's free-time cascade drops the element's last count and
#     `length` derefs a freed page. Needs no ownership adoption at all: it bites with
#     a PARAM container too, where no subtree is ever formed.
#
#   - THE SUBTREE-DROP FACE (counted, but frozen). `(get c 0)` as a NATIVE call takes
#     the Rule 5 pass-through retain, so under RC the reader holds its own reference
#     and the container may die. But `(push c r)` monomorphizes to the raw funnel and
#     ADOPTS `r` into `c`'s Owned subtree, which freezes `r`'s RC and makes that
#     retain inert — the container's release then subtree-drops `r` under the live
#     reader, and the reader's own value-resolved release faults resolving the freed
#     page even where its use does not. The ownership cut must refuse a subtree whose
#     member a read alias can still name — following the read CHAIN, since a read out
#     of an alias reaches deeper into the same subtree (adopt.md § "The lifetime
#     obligation the root carries") — and where the alias and the container die at the
#     SAME node, order the alias's page-reading release first (`order_releases`).
#
# DISTINCT FROM `region-container-read-escape-uaf`, where the read result leaves the
# activation — no lifetime or ordering inside the activation can bound that, so
# escape marks the container's contents escaping and the adopt never forms.
#
# A `%pop` REMOVE is deliberately NOT this shape: it extracts the element out of the
# container (and out of its subtree), so it is not a borrow — `region-pop-tail-
# moves-out-uaf` covers that path.

# The cascade face, through the opcode, with the read's result consumed by an
# enclosing call rather than a binding (so no binding chain covers the container).
(defn opcode-read-arg [i]
  (let [c @[]
        r (string "s" i)]
    (%array-push c r)
    (length (%get c 0))))

# The same, with the container arriving as a PARAM — no Fresh root, so the
# ownership forest forms no subtree and only the RC cascade is in play.
(defn opcode-read-param [c i]
  (let [r (string "s" i)]
    (%array-push c r)
    (length (%get c 0))))

# The subtree-drop face: a native `get`, its result bound and read after the
# container's last mention.
(defn native-read-bound [i]
  (let [c @[]
        r (string "s" i)]
    (push c r)
    (let [x (get c 0)]
      (length x))))

# The same through `first`, in tail position.
(defn native-first-tail [i]
  (let [c @[]
        r (string "s" i)]
    (push c r)
    (length (first c))))

# The SHARED-POINT face: the read's result is discarded, so the alias dies exactly
# where the container does and the intra-node release order alone decides. The
# container's release must come second — first, and its subtree drop reclaims the
# element the alias's own value-resolved release then reads.
(defn native-read-discarded [i]
  (let [c @[]
        r (string "s" i)]
    (push c r)
    (get c 0)
    i))

# The TRANSITIVE face: reading two levels deep. The inner read's alias dies at the
# outer read, so the container's own bound is satisfied — but the OUTER alias names a
# value inside the inner member, which the container's drop reclaims just the same.
# The alias obligation must follow the read chain, not stop at the first level.
(defn native-read-nested [i]
  (let [inner @[]
        s (string "s" i)
        c @[]]
    (push inner s)
    (push c inner)
    (let [x (get (get c 0) 0)]
      (length c)
      (length x))))

# The CONTROL: the container is mentioned again after the read, so its release
# already followed the borrow. Green before and after — it isolates the borrow from
# the shape.
(defn read-then-reuse [i]
  (let [c @[]
        r (string "s" i)]
    (push c r)
    (let [n (length (get c 0))]
      (+ n (length c)))))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (opcode-read-arg i)
    (opcode-read-param @[] i)
    (native-read-bound i)
    (native-first-tail i)
    (native-read-discarded i)
    (native-read-nested i)
    (read-then-reuse i)
    (assign i (+ i 1))))

# Prime: churn region ids so any freed page below is recycled onto a live region.
(churn 500)

# Steady-state region growth: the borrow extends the container's lifetime to the
# reader, which must not strand either region — both are freed once the reader is
# done, so region-count is bounded across the window.
(def r0 (arena/region-count))
(churn 500)
(def r1 (arena/region-count))
(def growth (- r1 r0))
(assert (< growth 50)
        (string "container-read borrow leaked " growth " regions over 500 ops"))

(println "region-container-read-borrow-uaf: ok")
