(elle/epoch 14)
# audited: 2026-10-05
# The ledger of tests/ratchet/valgrind.lisp: each program's definitely-lost bytes and error contexts under memcheck, on the release rig.
(producer "tests/ratchet/valgrind.lisp")
# The three error contexts are the boot's: `(+ 1 2)` alone reads them.
["boot and exit" :definitely-lost 0]
["boot and exit" :error-contexts 3]
["a fiber yields and resumes" :definitely-lost 0]
["a fiber yields and resumes" :error-contexts 3]
["a file read through the I/O backend" :definitely-lost 0]
["a file read through the I/O backend" :error-contexts 3]
["a function the JIT compiles" :definitely-lost 0]
["a function the JIT compiles" :error-contexts 3]
