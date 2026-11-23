(elle/epoch 18)
## audited: 2026-11-22
## A worker that loads an FFI library and exits with a thread-exit destructor still armed exits cleanly.
## tests/cli/overview.md
## docs/ffi.md
##
## The trap: a C library can register a destructor with `pthread_key_create`
## and arm it with `pthread_setspecific`, and glibc then calls that destructor,
## code in the library, as the thread exits. A library unmapped before a
## worker's thread exited would be called through an unmapped page, and the
## SIGSEGV would end the process.
##
## The counter-factual: a worker that deletes its key before it exits runs no
## destructor, so it exits cleanly whether or not the library stays mapped. Only
## a worker that leaves the destructor armed tells the two apart.
##
## `dlopen` maps the library executable, and the temp root may forbid that, so
## the fixture is built under target/, in a directory of the process's own,
## removed when the file ends. A box with no C compiler skips.

(include-file "../runner/lib/harness.lisp")

# reg_tls_dtor creates the key on its first call and arms it with a non-null
# value, so the destructor runs at thread exit; clear_tls_dtor deletes the key.
(def source
  (string "#include <pthread.h>\n"
          "static pthread_key_t key;\nstatic int armed = 0;\n"
          "static void destructor(void *unused) { (void)unused; }\n"
          "void reg_tls_dtor(void) {\n"
          "    if (!armed) { pthread_key_create(&key, destructor); armed = 1; }\n"
          "    pthread_setspecific(key, (void *)1);\n}\n"
          "void clear_tls_dtor(void) {\n"
          "    if (armed) { pthread_key_delete(key); armed = 0; }\n}\n"))

(def home (path/join (path/cwd) "target" (string "ffi-worker-" (sys/pid))))

(defn build []
  "Compile the fixture under HOME, and answer the library's path, or nil when
   no C compiler answers."
  (file/mkdir-all home)
  (write-fixture home "tlsdtor.c" source)
  (let [so (path/join home "libtlsdtor.so")
        [ok? r] (protect (subprocess/system "cc"
                           ["-shared" "-fPIC" "-O2" "-pthread" "-o" so
                            (path/join home "tlsdtor.c")]))]
    (if (and ok? (= r:exit 0)) so nil)))

(defn worker [so clear]
  "A worker body that loads SO and arms the destructor, then deletes the key
   when CLEAR holds."
  (string "(fn [] (def lib (ffi/native \"" so "\")) "
          "(def sig (ffi/signature :void @[])) "
          "(ffi/call (ffi/lookup lib \"reg_tls_dtor\") sig) "
          (if clear "(ffi/call (ffi/lookup lib \"clear_tls_dtor\") sig) " "")
          "0)"))

(defn exits-cleanly [program what]
  "Run PROGRAM, and require exit 0 with no signal. A worker whose ffi/native
   fails exits 1, and only its stderr says why."
  (let [r (elle-run ["-e" program] :env (env-without "RUST_MIN_STACK"))]
    (assert (= r:exit 0)
            (string what " exited " r:exit ". A negative exit is the signal "
                    "that says the library was unmapped before the worker's "
                    "thread exited; stderr: " r:err))))

(defer
  (file/delete-dir-all home)
  (let [so (build)]
    (gate! so "no C compiler to build the fixture" nil)

    (eprintln "scenario: a worker that leaves its destructor armed exits cleanly")
    (exits-cleanly (string "(sys/join (sys/spawn " (worker so false) "))")
                   "a worker that exited with its TLS destructor armed")
    (exits-cleanly (string "(sys/join (sys/spawn " (worker so true) "))")
                   "a worker that deleted its TLS key")

    # Two workers load one library, which the registry keys by its canonical
    # path, and both exit armed. Neither exit unmaps the code the other's
    # destructor runs.
    (eprintln "scenario: two workers of one library do not unmap it twice")
    (exits-cleanly (string "(def w1 (sys/spawn " (worker so false) ")) "
                           "(def w2 (sys/spawn " (worker so false) ")) "
                           "(sys/join w1) (sys/join w2)")
                   "two workers that loaded one FFI library")))

(eprintln "all ffi-worker scenarios passed")
