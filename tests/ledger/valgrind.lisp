(elle/epoch 15)
# audited: 2026-10-12
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# Every error context here is the cross-region scan's page-header probe. It
# reads the size tag at each power-of-two base below a code header's payload,
# so where a payload sits in its page decides whether the probe reads
# uninitialized bytes, and a change to a payload's layout moves these counts.
# On the compiled path `(+ 1 2)` alone reads no error context. On the cached
# path it reads two: the probe under the header the cache load allocates, and
# under `MakeClosure`'s. Each program reads what the boot reads on its path.
["boot and exit, stdlib compiled" :definitely-lost 0]
["boot and exit, stdlib compiled" :error-contexts 0]
["a fiber yields and resumes, stdlib compiled" :definitely-lost 0]
["a fiber yields and resumes, stdlib compiled" :error-contexts 0]
["a file read through the I/O backend, stdlib compiled" :definitely-lost 0]
["a file read through the I/O backend, stdlib compiled" :error-contexts 0]
["a function the JIT compiles, stdlib compiled" :definitely-lost 0]
["a function the JIT compiles, stdlib compiled" :error-contexts 0]
["boot and exit, stdlib cached" :definitely-lost 0]
["boot and exit, stdlib cached" :error-contexts 2]
["a fiber yields and resumes, stdlib cached" :definitely-lost 0]
["a fiber yields and resumes, stdlib cached" :error-contexts 2]
["a file read through the I/O backend, stdlib cached" :definitely-lost 0]
["a file read through the I/O backend, stdlib cached" :error-contexts 2]
["a function the JIT compiles, stdlib cached" :definitely-lost 0]
["a function the JIT compiles, stdlib cached" :error-contexts 2]
