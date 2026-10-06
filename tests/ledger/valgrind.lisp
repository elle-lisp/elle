(elle/epoch 14)
# audited: 2026-10-06
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# On each path `(+ 1 2)` alone reads three error contexts. The two paths share
# one of them, and each path has two of its own.
["boot and exit, stdlib compiled" :definitely-lost 0]
["boot and exit, stdlib compiled" :error-contexts 3]
["a fiber yields and resumes, stdlib compiled" :definitely-lost 0]
["a fiber yields and resumes, stdlib compiled" :error-contexts 3]
["a file read through the I/O backend, stdlib compiled" :definitely-lost 0]
["a file read through the I/O backend, stdlib compiled" :error-contexts 3]
["a function the JIT compiles, stdlib compiled" :definitely-lost 0]
# On this path the JIT program reads one site more than the boot.
["a function the JIT compiles, stdlib compiled" :error-contexts 4]
["boot and exit, stdlib cached" :definitely-lost 0]
["boot and exit, stdlib cached" :error-contexts 3]
["a fiber yields and resumes, stdlib cached" :definitely-lost 0]
["a fiber yields and resumes, stdlib cached" :error-contexts 3]
["a file read through the I/O backend, stdlib cached" :definitely-lost 0]
["a file read through the I/O backend, stdlib cached" :error-contexts 3]
["a function the JIT compiles, stdlib cached" :definitely-lost 0]
["a function the JIT compiles, stdlib cached" :error-contexts 3]
