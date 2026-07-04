(elle/epoch 12)
# A multi-form file whose SHARED SETUP is gated on an absent dependency.
#
# `dep` simulates a missing optional library — an `:ffi-error` that the import
# site re-raises as a loud `:gated` so the runner skips (not fails) the file.
#
# This is a multi-form file, so it compiles as ONE whole-file thunk
# (compile/whole-module): `dep` runs FIRST inside that thunk and raises `:gated`,
# aborting the file before the asserts. Contract (docs/test-runner.md § Gating):
# the runner records a runtime skip carrying the reason (the whole-file form,
# idx 0); exit stays 0 (a skip is not a failure). A genuine setup error would
# instead fail — this fixture pins the skip.
(def dep
  (let [r (protect (error (struct :error :ffi-error
                                  :message "cannot open shared object file")))]
    (if (get r 0)
      (get r 1)
      (if (= (get (get r 1) :error) :ffi-error)
        (error (struct :error :gated :reason "libfixture.so not installed"))
        (error (get r 1))))))

# These never run — the eager `dep` gates first, aborting the compile. They make
# the file look like a real test file (had it not gated, these would be thunks).
(assert (= (unbox dep) 1) "uses dep: alpha")
(assert (= (+ 1 1) 2) "beta")
(assert (= (* 2 3) 6) "gamma")
