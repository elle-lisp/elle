(elle/epoch 18)
# audited: 2026-11-26
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# An error context on either path is the cross-region scan's page-header
# probe reading a byte nothing wrote. The probe reads the size tag at each
# power-of-two base below a code header's payload. A record copied into a
# code region with its padding publishes that padding to the probe, and so
# does an object slot whose short variant lies across such a base
# (docs/impl/region/generations.md). The hydrated path reads the code the
# image carries, and the compiled path what the emitter writes.
["boot and exit, stdlib compiled" :definitely-lost 0]
["boot and exit, stdlib compiled" :error-contexts 0]
["a fiber yields and resumes, stdlib compiled" :definitely-lost 0]
["a fiber yields and resumes, stdlib compiled" :error-contexts 0]
["a file read through the I/O backend, stdlib compiled" :definitely-lost 0]
["a file read through the I/O backend, stdlib compiled" :error-contexts 0]
["a function the JIT compiles, stdlib compiled" :definitely-lost 0]
["a function the JIT compiles, stdlib compiled" :error-contexts 0]
["boot and exit, stdlib hydrated" :definitely-lost 0]
["boot and exit, stdlib hydrated" :error-contexts 0]
["a fiber yields and resumes, stdlib hydrated" :definitely-lost 0]
["a fiber yields and resumes, stdlib hydrated" :error-contexts 0]
["a file read through the I/O backend, stdlib hydrated" :definitely-lost 0]
["a file read through the I/O backend, stdlib hydrated" :error-contexts 0]
["a function the JIT compiles, stdlib hydrated" :definitely-lost 0]
["a function the JIT compiles, stdlib hydrated" :error-contexts 0]
