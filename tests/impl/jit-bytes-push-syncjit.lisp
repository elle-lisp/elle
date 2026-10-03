(elle/epoch 13)
# audited: 2026-09-29
# jit-bytes-push.lisp with every function compiled on its first call, on the VM thread.
# docs/impl/jit.md
#
# `repeat2` is compiled before it first passes a hot function along, so each
# later call reaches that function through `elle_jit_call`, and
# `(jit? push-imm-hot)` says whether the compiled call path promotes what it
# calls. The background worker would leave `repeat2` interpreted long enough
# for its callees to be counted on the interpreter's path.

(include-file "jit-bytes-push.lisp")
