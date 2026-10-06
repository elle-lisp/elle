(elle/epoch 14)
# audited: 2026-10-06
# meta/location answers the source location of the form itself, and a form a macro builds answers the macro call's.
# docs/modules.md

(def here (meta/location))

(assert (struct? here) "the location is a struct")
(assert (= "meta-location.lisp" (path/filename here:file))
        "the file is the one that holds the form")
(assert (path/absolute? here:file)
        "the file is absolute, so it names one file wherever the process started")
(assert (= here:file (path/normalize here:file)) "the file is normalized")

# Exact lines move when the formatter rewrites the file, so compare two forms.
(def first-line (meta/location))
(def second-line (meta/location))
(assert (= (+ 1 first-line:line) second-line:line)
        "each form answers its own line")
(assert (= first-line:col second-line:col)
        "two forms at one indentation answer one column")

# The macro comes from another file. Its expansion answers this file, because
# a form a macro builds takes the location of the macro call. The
# counter-factual reads the location off the template's symbol, which names
# where.lisp.
(include-file "../modules/where.lisp")
(def [from-macro beside] [(where) (meta/location)])
(assert (= here:file from-macro:file)
        "a macro-built form answers the caller's file")
(assert (= beside:line from-macro:line) "and the caller's line")

# A datum that eval compiles has no source file.
(assert (nil? (get (eval '(meta/location)) :file))
        "code with no file answers a nil file")
