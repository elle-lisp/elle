(elle/epoch 12)
# audited: 2026-09-29
# Twenty thousand nested fiber/resume calls complete without consuming the host call stack.
# docs/impl/vm.md
#
# Deep fiber/resume nesting must not consume the host call stack: fibers are
# the language's concurrency primitive, and Rust stack depth must stay
# constant in nesting depth. Nested resumes route through the SIG_SWITCH
# trampoline (`finish_fiber_resume`, src/vm/fiber/trampoline.rs): the calling
# fiber parks its continuation frame and hands the child to
# `pending_fiber_resume`; the trampoline descends iteratively and its unwind
# loop resumes each parent with its child's result. 20000 levels is enough to
# overflow an 8 MiB stack if any per-level Rust frames survive.
#
# The sidecar keeps the JIT off, and the suite's eager profile runs the file
# again with every function compiled on its first call. Two driver shapes run
# in each: `nest-add`/`nest-tail` create each child inline, which carries a
# MakeClosure the JIT declines, so they pin the interpreter path even in the
# eager pass. The `-jit` twins hoist the `fiber/new` into a helper, so the
# recursive resume caller itself is JIT-admissible and the eager pass drives a
# compiled caller through the suspend machinery.

# Non-tail resume: each level adds 1 to its child's result, so the final
# value also proves every continuation frame resumed with the right value.
(defn nest-add [n]
  (if (= n 0)
    0
    (let [child (fiber/new (fn [] (nest-add (- n 1))) |:error|)]
      (+ 1 (fiber/resume child)))))
(assert (= (nest-add 20000) 20000) "deep non-tail fiber/resume completes")

# Tail resume: the resume is the fiber body's final expression, so each
# fiber's result IS its child's result (the empty-continuation path).
(defn nest-tail [n]
  (if (= n 0)
    :done
    (let [child (fiber/new (fn [] (nest-tail (- n 1))) |:error|)]
      (fiber/resume child))))
(assert (= (nest-tail 20000) :done) "deep tail fiber/resume completes")

# JIT-admissible drivers: the fiber creation (MakeClosure) lives in a
# helper, so the recursive resume caller compiles and the eager pass
# exercises a compiled fiber/resume caller at depth.
(defn make-add-child [n]
  (fiber/new (fn [] (nest-add-jit n)) |:error|))
(defn nest-add-jit [n]
  (if (= n 0)
    0
    (+ 1 (fiber/resume (make-add-child (- n 1))))))
(assert (= (nest-add-jit 20000) 20000)
        "deep non-tail resume from a compilable driver completes")

(defn make-tail-child [n]
  (fiber/new (fn [] (nest-tail-jit n)) |:error|))
(defn nest-tail-jit [n]
  (if (= n 0)
    :done
    (fiber/resume (make-tail-child (- n 1)))))
(assert (= (nest-tail-jit 20000) :done)
        "deep tail resume from a compilable driver completes")

(println "fiber-depth: OK")
