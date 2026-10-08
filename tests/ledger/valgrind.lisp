(elle/epoch 14)
# audited: 2026-10-07
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# On the compiled path `(+ 1 2)` alone reads four error contexts: two in the
# analyzer's `as_list`, and the cross-region scan's page-header probe under two
# allocations of a code header, the emitter's and `MakeClosure`'s. On the cached
# path it reads the probe once, under the header the cache load allocates. Each
# program reads what the boot reads on its path.
["boot and exit, stdlib compiled" :definitely-lost 0]
["boot and exit, stdlib compiled" :error-contexts 4]
["a fiber yields and resumes, stdlib compiled" :definitely-lost 0]
["a fiber yields and resumes, stdlib compiled" :error-contexts 4]
["a file read through the I/O backend, stdlib compiled" :definitely-lost 0]
["a file read through the I/O backend, stdlib compiled" :error-contexts 4]
["a function the JIT compiles, stdlib compiled" :definitely-lost 0]
["a function the JIT compiles, stdlib compiled" :error-contexts 4]
["boot and exit, stdlib cached" :definitely-lost 0]
["boot and exit, stdlib cached" :error-contexts 1]
["a fiber yields and resumes, stdlib cached" :definitely-lost 0]
["a fiber yields and resumes, stdlib cached" :error-contexts 1]
["a file read through the I/O backend, stdlib cached" :definitely-lost 0]
["a file read through the I/O backend, stdlib cached" :error-contexts 1]
["a function the JIT compiles, stdlib cached" :definitely-lost 0]
["a function the JIT compiles, stdlib cached" :error-contexts 1]
