# Tests for fiber primitives (FiberHandle, child chain, propagate, cancel)
#
# These tests verify fiber operations including:
# - Fiber child chain wiring
# - Fiber propagate operations
# - Fiber resume/cancel mechanics
# - Fiber status tracking
# - Fiber identity preservation
# - Fiber signal handling
# - Fiber closure parameters

(import-file "./examples/assertions.lisp")

# ============================================================================
# Fiber child chain wiring
# ============================================================================

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 0)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume inner)
                     42)
                   1)))
      (fiber/resume outer)
      (fiber? (fiber/child outer))))
  true
  "fiber/child returns fiber after uncaught signal propagation")

(assert-eq
  (let ((f (fiber/new (fn () 42) 0)))
    (fiber/child f))
  nil
  "fiber/child is nil before resume")

# ============================================================================
# Fiber propagate operations
# ============================================================================

(assert-err
  (fn ()
    (let ((inner (fiber/new (fn () (fiber/signal 1 "boom")) 1)))
      (fiber/resume inner)
      (fiber/propagate inner)))
  "fiber/propagate error propagates to caller")

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 2 99)) 2)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   2)))
      (fiber/resume outer)))
  99
  "fiber/propagate yield is caught by outer mask")

(assert-err
  (fn ()
    (let ((f (fiber/new (fn () 42) 0)))
      (fiber/resume f)
      (fiber/propagate f)))
  "fiber/propagate dead fiber errors")

# ============================================================================
# Fiber cancel operations
# ============================================================================

(assert-eq
  (keyword->string
    (let ((f (fiber/new (fn () (fiber/signal 2 "waiting") 99) 3)))
      (fiber/resume f)
      (fiber/cancel f "cancelled")
      (fiber/status f)))
  "error"
  "fiber/cancel suspended fiber sets error status")

(assert-eq
  (keyword->string
    (let ((f (fiber/new (fn () 42) 1)))
      (fiber/cancel f "never started")
      (fiber/status f)))
  "error"
  "fiber/cancel new fiber sets error status")

(assert-err
  (fn ()
    (let ((f (fiber/new (fn () 42) 0)))
      (fiber/resume f)
      (fiber/cancel f "too late")))
  "fiber/cancel dead fiber errors")

(assert-eq
  (let ((f (fiber/new (fn () 42) 1)))
    (fiber/cancel f "injected")
    (fiber/value f))
  "injected"
  "fiber/cancel returns error value when caught")

# ============================================================================
# Error macro arity
# ============================================================================

(assert-eq
  (try (error) (catch e e))
  nil
  "error with no args defaults to nil")

(assert-true
  (keyword? (try (error :boom) (catch e e)))
  "error with value works")

# ============================================================================
# Fiber cancel default nil and cancel alias
# ============================================================================

(assert-eq
  (let ((f (fiber/new (fn () 42) 1)))
    (fiber/cancel f)
    (fiber/value f))
  nil
  "fiber/cancel with 1 arg defaults error value to nil")

(assert-eq
  (let ((f (fiber/new (fn () 42) 1)))
    (cancel f "stopped")
    (fiber/value f))
  "stopped"
  "cancel alias for fiber/cancel works")

(assert-eq
  (let ((f (fiber/new (fn () 42) 1)))
    (cancel f)
    (fiber/value f))
  nil
  "cancel alias with 1 arg defaults to nil")

# ============================================================================
# Basic fiber resume
# ============================================================================

(assert-eq
  (let ((f (fiber/new (fn () 42) 0)))
    (fiber/resume f))
  42
  "fiber/resume basic")

(assert-eq
  (let ((f (fiber/new (fn () (fiber/signal 2 10) 20) 2)))
    (+ (fiber/resume f) (fiber/resume f)))
  30
  "fiber/yield and resume")

(assert-true
  (not (let (([ok? _] (protect (fn ()
                                 (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 1)))
                                   (fiber/resume f))))))
         (not ok?)))
  "fiber/error caught by mask")

(assert-err
  (fn ()
    (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 0)))
      (fiber/resume f)))
  "fiber/error propagates without mask")

# ============================================================================
# Fiber propagate preserving child chain
# ============================================================================

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 1)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   1)))
      (fiber/resume outer)
      (fiber? (fiber/child outer))))
  true
  "fiber/propagate preserves child chain")

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 2 99)) 2)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume inner)
                     (fiber/propagate inner))
                   2)))
      (fiber/resume outer)
      (identical? inner (fiber/child outer))))
  true
  "fiber/child after propagate returns identical fiber")

# ============================================================================
# Fiber resume and cancel in tail position
# ============================================================================

(assert-eq
  (let ((inner (fiber/new (fn () 42) 0)))
    (let ((outer (fiber/new (fn () (fiber/resume inner)) 0)))
      (fiber/resume outer)))
  42
  "fiber/resume in tail position")

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 2 10) 20) 2)))
    (let ((outer (fiber/new (fn () (fiber/resume inner)) 0)))
      (fiber/resume outer)))
  10
  "fiber/resume yield in tail position")

(assert-true
  (not (let (([ok? _] (protect (fn ()
                                 (let ((target (fiber/new (fn () 42) 1)))
                                   (let ((canceller (fiber/new
                                                      (fn () (fiber/cancel target "cancelled"))
                                                      0)))
                                     (fiber/resume canceller)))))))
         (not ok?)))
  "fiber/cancel in tail position")

(assert-eq
  (keyword->string
    (let ((target (fiber/new (fn () (fiber/signal 2 0) 99) 3)))
      (fiber/resume target)
      (let ((canceller (fiber/new
                         (fn () (fiber/cancel target "stop"))
                         0)))
        (fiber/resume canceller))
      (fiber/status target)))
  "error"
  "fiber/cancel suspended in tail position sets error status")

# ============================================================================
# 3-level nested fiber resume
# ============================================================================

(assert-eq
  (let ((c (fiber/new (fn () (fiber/signal 2 10)) 2)))
    (let ((b (fiber/new
               (fn ()
                 (+ (fiber/resume c) 5))
               0)))
      (let ((a (fiber/new
                 (fn ()
                   (+ (fiber/resume b) 1))
                 0)))
        (fiber/resume a))))
  16
  "three level nested fiber resume")

(assert-true
  (not (let (([ok? _] (protect (fn ()
                                 (let ((c (fiber/new (fn () (fiber/signal 1 "deep error")) 0)))
                                   (let ((b (fiber/new
                                              (fn () (fiber/resume c))
                                              0)))
                                     (let ((a (fiber/new
                                                (fn () (fiber/resume b))
                                                1)))
                                       (fiber/resume a))))))))
         (not ok?)))
  "three level nested fiber error propagation")

# ============================================================================
# Fiber parent and child identity
# ============================================================================

(assert-eq
  (let ((f (fiber/new (fn () 42) 0)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume f)
                     42)
                   0)))
      (fiber/resume outer)
      (identical? (fiber/parent f) (fiber/parent f))))
  true
  "fiber/parent returns identical values")

(assert-eq
  (let ((inner (fiber/new (fn () (fiber/signal 1 "err")) 0)))
    (let ((outer (fiber/new
                   (fn ()
                     (fiber/resume inner)
                     42)
                   1)))
      (fiber/resume outer)
      (identical? (fiber/child outer) (fiber/child outer))))
  true
  "fiber/child returns identical values")

# ============================================================================
# Caught SIG_ERROR status and resumability (#299)
# ============================================================================

(assert-eq
  (keyword->string
    (let ((f (fiber/new (fn () (fiber/signal 1 "oops") "recovered") 1)))
      (fiber/resume f)
      (fiber/status f)))
  "suspended"
  "caught SIG_ERROR leaves fiber suspended")

(assert-eq
  (let ((f (fiber/new (fn () (fiber/signal 1 "oops") "recovered") 1)))
    (fiber/resume f)
    (fiber/resume f))
  "recovered"
  "caught SIG_ERROR fiber is resumable")

(assert-err
  (fn ()
    (let ((f (fiber/new (fn () (fiber/signal 1 "oops")) 0)))
      (fiber/resume f)))
  "uncaught SIG_ERROR produces error")

(assert-eq
  (keyword->string
    (let ((f (fiber/new (fn () (fiber/signal 2 "waiting") 99) 3)))
      (fiber/resume f)
      (fiber/cancel f "stop")
      (fiber/status f)))
  "error"
  "cancel always produces error status")

# ============================================================================
# Fiber with signal parameter (#346)
# ============================================================================

(assert-eq
  (let ((f (fiber/new (fn (s) (+ s 42)) 0)))
    (fiber/resume f 8))
  50
  "fiber closure with signal parameter")

(assert-eq
  (let ((f (fiber/new (fn (s) (fiber/signal s 42)) 2)))
    (fiber/resume f 2)
    (fiber/value f))
  42
  "fiber signal parameter with valid bits")

(assert-eq
  (keyword->string
    (let ((f (fiber/new (fn (s) (fiber/signal s 42)) 1)))
      (fiber/resume f)
      (fiber/status f)))
  "suspended"
  "fiber param nil default no panic")

(assert-eq
  (let ((f (fiber/new (fn (x) (* x x)) 0)))
    (fiber/resume f 7))
  49
  "fiber closure with resume value as parameter")

(assert-eq
  (let ((f (fiber/new (fn () 42) 0)))
    (fiber/resume f))
  42
  "fiber zero param closure still works")

# ============================================================================
# Issue #415: letrec binding survives fiber yield/resume
# ============================================================================

(assert-eq
  (let* ((f (fiber/new (fn ()
                  (letrec ((go (fn (n)
                              (fiber/signal 2 n)
                              (go (+ n 1)))))
                    (go 0)))
              2)))
    (list (fiber/resume f) (fiber/resume f) (fiber/resume f)))
  (list 0 1 2)
  "letrec binding survives fiber yield/resume")

(assert-eq
  (begin
    (defn helper (n)
      (fiber/signal 2 n)
      (helper (+ n 10)))
    (let* ((f (fiber/new (fn () (helper 1)) 2)))
      (list (fiber/resume f) (fiber/resume f) (fiber/resume f))))
  (list 1 11 21)
  "tail call then signal preserves state")

(assert-eq
  (begin
    (defn signaler (n) (fiber/signal 2 n) (signaler (+ n 1)))
    (defn bouncer (n) (signaler n))
    (let* ((f (fiber/new (fn () (bouncer 100)) 2)))
      (list (fiber/resume f) (fiber/resume f))))
  (list 100 101)
  "multiple tail calls before signal")
