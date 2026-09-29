(elle/epoch 13)
# audited: 2026-09-29
# The language test recur-as-value.lisp, run with the use-after-free oracle armed.
# docs/impl/region/diagnostics.md
#
# The language test asserts the values a stale self-reference would get
# wrong. The sidecar arms guardfree, so a release that freed the live closure
# or its environment faults on the read instead of reading a recycled page.

(include-file "../lang/recur-as-value.lisp")
