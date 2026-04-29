(elle/epoch 9)
# BLAS/LAPACK FFI demo — optimized linear algebra via CBLAS and LAPACKE.
# Demonstrates ffi/defbind, ffi/array marshalling, and defer-based cleanup.

# ── Libraries ─────────────────────────────────────────────────────
(def cblas (ffi/native "libcblas.so.3"))
(def lapacke (ffi/native "liblapacke.so.3"))

# ── CBLAS constants ───────────────────────────────────────────────
(def CblasRowMajor 101)
(def CblasNoTrans 111)

# ── CBLAS bindings ────────────────────────────────────────────────
(ffi/defbind cblas-ddot cblas "cblas_ddot" :double @[:int :ptr :int :ptr :int])

(ffi/defbind cblas-dgemv cblas "cblas_dgemv"
             :void @[:int :int :int :int :double :ptr :int :ptr :int :double
                     :ptr :int])

(ffi/defbind cblas-dgemm cblas "cblas_dgemm"
             :void @[:int :int :int :int :int :int :double :ptr :int :ptr :int
                     :double :ptr :int])

# ── LAPACKE bindings ─────────────────────────────────────────────
(ffi/defbind lapacke-dgesv lapacke "LAPACKE_dgesv"
             :int @[:int :int :int :ptr :int :ptr :ptr :int])

# ── Helpers ───────────────────────────────────────────────────────
(defn alloc-doubles [lst]
  "Allocate C memory and write an Elle list of doubles to it."
  (let* [n (length lst)
         arr-type (ffi/array :double n)
         ptr (ffi/malloc (ffi/size arr-type))]
    (ffi/write ptr arr-type (apply array lst))
    ptr))

# ── 1. DDOT: dot product ─────────────────────────────────────────
(println "=== CBLAS DDOT (Dot Product) ===")

(let* [x '(1.0 2.0 3.0)
       y '(4.0 5.0 6.0)
       x-ptr (alloc-doubles x)
       y-ptr (alloc-doubles y)]
  (defer
    (ffi/free x-ptr)
    (defer
      (ffi/free y-ptr)
      (let* [result (cblas-ddot 3 x-ptr 1 y-ptr 1)]
        (println "x = " x)
        (println "y = " y)
        (println "dot(x, y) = " result)
        (println "expected: 32.0 (1*4 + 2*5 + 3*6)")))))

(println)

# ── 2. DGEMV: matrix-vector multiply ─────────────────────────────
(println "=== CBLAS DGEMV (Matrix-Vector Multiply) ===")

# A = [[1,2,3],[4,5,6]] (2x3 row-major), x = [1,2,3]
# y := A*x = [14, 32]
(let* [m 2
       n 3
       a-ptr (alloc-doubles '(1.0 2.0 3.0 4.0 5.0 6.0))
       x-ptr (alloc-doubles '(1.0 2.0 3.0))
       y-ptr (alloc-doubles '(0.0 0.0))]
  (defer
    (ffi/free a-ptr)
    (defer
      (ffi/free x-ptr)
      (defer
        (ffi/free y-ptr)
        (cblas-dgemv CblasRowMajor CblasNoTrans m n 1.0 a-ptr n x-ptr 1 0.0
                     y-ptr 1)
        (let* [result (ffi/read y-ptr (ffi/array :double m))]
          (println "A = [[1,2,3],[4,5,6]]")
          (println "x = [1,2,3]")
          (println "y = A*x = " result)
          (println "expected: [14.0, 32.0]"))))))

(println)

# ── 3. DGEMM: matrix-matrix multiply ─────────────────────────────
(println "=== CBLAS DGEMM (Matrix-Matrix Multiply) ===")

# A(2x3) * B(3x2) = C(2x2)
(let* [m 2
       n 2
       k 3
       a-ptr (alloc-doubles '(1.0 2.0 3.0 4.0 5.0 6.0))
       b-ptr (alloc-doubles '(1.0 2.0 3.0 4.0 5.0 6.0))
       c-ptr (alloc-doubles '(0.0 0.0 0.0 0.0))]
  (defer
    (ffi/free a-ptr)
    (defer
      (ffi/free b-ptr)
      (defer
        (ffi/free c-ptr)
        (cblas-dgemm CblasRowMajor CblasNoTrans CblasNoTrans m n k 1.0 a-ptr k
                     b-ptr n 0.0 c-ptr n)
        (let* [result (ffi/read c-ptr (ffi/array :double (* m n)))]
          (println "A = [[1,2,3],[4,5,6]]")
          (println "B = [[1,2],[3,4],[5,6]]")
          (println "C = A*B = " result)
          (println "expected: [22.0, 28.0, 49.0, 64.0]"))))))

(println)

# ── 4. DGESV: solve linear system A*X = B ────────────────────────
(println "=== LAPACKE DGESV (Linear System Solve) ===")

# A = [[2,1],[1,2]], B = [3,3] => X = [1,1]
(let* [n 2
       a-ptr (alloc-doubles '(2.0 1.0 1.0 2.0))
       b-ptr (alloc-doubles '(3.0 3.0))
       ipiv-type (ffi/array :int n)
       ipiv-ptr (ffi/malloc (ffi/size ipiv-type))]
  (defer
    (ffi/free a-ptr)
    (defer
      (ffi/free b-ptr)
      (defer
        (ffi/free ipiv-ptr)
        (let* [info (lapacke-dgesv CblasRowMajor n 1 a-ptr n ipiv-ptr b-ptr 1)
               result (ffi/read b-ptr (ffi/array :double n))]
          (println "Solve A*X = B where A = [[2,1],[1,2]], B = [3,3]")
          (println "X = " result)
          (println "expected: [1.0, 1.0]")
          (println "info = " info " (0 = success)"))))))

(println)
(println "=== All BLAS/LAPACK demos completed successfully ===")
