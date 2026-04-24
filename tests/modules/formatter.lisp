(elle/epoch 9)
# Parametric string formatter module
# Accepts :prefix, :suffix, :separator keyword configuration
# Returns a struct of exported functions that close over the config

(fn (&named prefix suffix separator)
  "Parametric formatter module. Accepts :prefix, :suffix, :separator."
  (let* [prefix (if (nil? prefix) "" prefix)
         suffix (if (nil? suffix) "" suffix)
         separator (if (nil? separator) ", " separator)]

    (defn wrap [s]
      "Wrap a string with the configured prefix and suffix."
      (-> prefix (append s) (append suffix)))

    (defn join [items]
      "Join array elements with the configured separator."
      (string/join (map string items) separator))

    (defn upper [s]
      "Convert a string to uppercase (not configuration-dependent)."
      (string/upcase s))

    (defn identity [x]
      "Return the argument unchanged."
      x)

    {:wrap wrap :join join :upper upper :identity identity}))
