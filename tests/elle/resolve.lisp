(elle/epoch 6)
# Module resolution tests

# ============================================================================
# import-file still works (backward compatibility)
# ============================================================================

(assert (not (nil? (import-file "lib/http.lisp"))) "import-file loads lib/http.lisp")

# ============================================================================
# import resolves bare names via ELLE_PATH
# ============================================================================

(assert (not (nil? (import "http"))) "import bare name finds lib/http.lisp")

# ============================================================================
# import error on nonexistent module
# ============================================================================

(let [[[ok? _] (protect ((fn [] (import "nonexistent-xyz-module-42"))))]]
  (assert (not ok?) "import nonexistent module errors"))

# ============================================================================
# import with explicit extension
# ============================================================================

(assert (not (nil? (import "http.lisp"))) "import with .lisp extension")
