(elle/epoch 15)
# audited: 2026-11-18
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
# in each. `nest-add`/`nest-tail` take a `&named` parameter, which the JIT
# refuses, so they pin the interpreter path even in the eager pass. Their
# `-jit` twins are the same drivers without it, so the eager pass drives a
# compiled caller, which builds each child's closure, through the suspend
# machinery.

# Non-tail resume: each level adds 1 to its child's result, so the final
# value also proves every continuation frame resumed with the right value.
(defn nest-add [n &named unused]
  (if (= n 0)
    0
    (let [child (fiber/new (fn [] (nest-add (- n 1))) |:error|)]
      (+ 1 (fiber/resume child)))))
(assert (= (nest-add 20000) 20000) "deep non-tail fiber/resume completes")

# Tail resume: the resume is the fiber body's final expression, so each
# fiber's result IS its child's result (the empty-continuation path).
(defn nest-tail [n &named unused]
  (if (= n 0)
    :done
    (let [child (fiber/new (fn [] (nest-tail (- n 1))) |:error|)]
      (fiber/resume child))))
(assert (= (nest-tail 20000) :done) "deep tail fiber/resume completes")

# Compiled drivers: the eager pass runs each as compiled code that builds its
# child's closure and resumes it.
(defn nest-add-jit [n]
  (if (= n 0)
    0
    (let [child (fiber/new (fn [] (nest-add-jit (- n 1))) |:error|)]
      (+ 1 (fiber/resume child)))))
(assert (= (nest-add-jit 20000) 20000)
        "deep non-tail resume from a compiled driver completes")

(defn nest-tail-jit [n]
  (if (= n 0)
    :done
    (let [child (fiber/new (fn [] (nest-tail-jit (- n 1))) |:error|)]
      (fiber/resume child))))
(assert (= (nest-tail-jit 20000) :done)
        "deep tail resume from a compiled driver completes")

(println "fiber-depth: OK")
