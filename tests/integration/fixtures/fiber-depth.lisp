(elle/epoch 12)
# tests/integration/fixtures/fiber-depth.lisp
#
# Deep fiber/resume nesting must not consume the host call stack: fibers are
# the language's concurrency primitive, and "the primitive crashes the
# process at depth" is a crash bug, not a limitation. Each level here spawns
# a child fiber whose body resumes one level deeper, 20000 deep — enough to
# overflow an 8 MiB Rust stack when every nesting level costs Rust frames
# (do_fiber_resume → with_child_fiber → execute_bytecode → handler → recurse).
#
# The fix routes nested `fiber/resume` through the existing SIG_SWITCH
# trampoline in `do_fiber_resume` (src/vm/fiber.rs): the calling fiber
# suspends with its continuation frame and hands the child to
# `pending_fiber_resume`; the trampoline descends iteratively and the unwind
# loop resumes each parent with its child's result. Rust stack depth stays
# constant in nesting depth.
#
# Quarantined here — NOT under tests/elle/ — because (a) while the defect is
# live the witness is a process-fatal stack-overflow abort that would take
# the shared `make smoke` harness down, and (b) the two pins in
# tests/integration/elle_scripts.rs run it under the process-global
# `--jit=off` / `--jit=eager` modes separately: the bytecode-VM path is
# trampolined; the JIT resume path (`handle_fiber_resume_signal_jit`) still
# recurses and remains a RED pin until it gets the same treatment.

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

(println "fiber-depth: OK")
