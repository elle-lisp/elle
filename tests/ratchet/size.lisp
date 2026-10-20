(elle/epoch 15)
# audited: 2026-10-20
# Every source and document over its cap at its 500-token bucket, and the excess they carry, read against tests/ledger/size-*.lisp.
# tests/ratchet/overview.md
#
# The script lists the files under the working directory, which is the
# repository root, as it is for every path the runner takes.

(def r ((import "std/ratchet")))

# A text reads the same on every build, and the rows are one build's.
(unless (= (elle/build) "jit-uring-linux-x86_64")
  (error {:error :gated
          :reason (string "the rig is " (elle/build)
                          ", and the rows are jit-uring-linux-x86_64's")}))

(include-file "size/count.lisp")

(defn listed [flag]
  "The paths scripts/audit FLAG prints, one per line."
  (let [run (subprocess/system "scripts/audit" [flag])]
    (assert (= (get run :exit) 0)
            (string "scripts/audit " flag " exits 0: " (get run :stderr)))
    (filter (fn [path] (not (empty? path)))
            (string/split (get run :stdout) "\n"))))

(def @priced @[])
(each path in (concat (listed "--sources") (listed "--documents"))
  (let [value (reading path (elle/tokens (slurp path)))]
    (when value
      (r:read path :tokens value)
      (push priced [path value]))))

(let [split (excess priced tables)]
  (r:read "the excess the work sheds" :tokens (get split :sheds))
  (r:read "the excess tables carry" :tokens (get split :tables)))
