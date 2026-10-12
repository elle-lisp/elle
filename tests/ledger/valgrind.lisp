(elle/epoch 15)
# audited: 2026-10-12
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# Every program reads one error context: the cross-region scan's page-header
# probe, under the allocation of a code header. On the compiled path the
# emitter and `MakeClosure` each allocate one; their errors share a context,
# because the store's resolver frame makes their top four frames alike. On the
# cached path the header the cache load allocates is the one.
["boot and exit, stdlib compiled" :definitely-lost 0]
["boot and exit, stdlib compiled" :error-contexts 1]
["a fiber yields and resumes, stdlib compiled" :definitely-lost 0]
["a fiber yields and resumes, stdlib compiled" :error-contexts 1]
["a file read through the I/O backend, stdlib compiled" :definitely-lost 0]
["a file read through the I/O backend, stdlib compiled" :error-contexts 1]
["a function the JIT compiles, stdlib compiled" :definitely-lost 0]
["a function the JIT compiles, stdlib compiled" :error-contexts 1]
["boot and exit, stdlib cached" :definitely-lost 0]
["boot and exit, stdlib cached" :error-contexts 1]
["a fiber yields and resumes, stdlib cached" :definitely-lost 0]
["a fiber yields and resumes, stdlib cached" :error-contexts 1]
["a file read through the I/O backend, stdlib cached" :definitely-lost 0]
["a file read through the I/O backend, stdlib cached" :error-contexts 1]
["a function the JIT compiles, stdlib cached" :definitely-lost 0]
["a function the JIT compiles, stdlib cached" :error-contexts 1]
