(elle/epoch 18)
# audited: 2026-11-19
# The plugin gate of the std/gun tests: a plugin that did not load skips the whole file, with its reason.
# docs/stdlib/gun.md
# tests/lang/overview.md
#
# It reads no name of the file that includes it, so that file includes it
# first, and gates the plugins its other includes read.

# The import stays literal in the test file, because the plugin checks of the
# suite read each (import "plugin/X") there.
(defn plugin [name loaded]
  "The plugin that (protect (import \"plugin/NAME\")) loaded, or a gated skip
   of the whole file when it did not load."
  (let [[ok? p] loaded]
    (unless ok?
      (error {:error :gated :reason (string "plugin/" name " not available")}))
    p))
