# BLAS/LAPACK FFI Demo
# ============================================================================
# 
# Demonstrates calling optimized numerical routines from Elle via FFI.
# 
# CBLAS (C interface to BLAS) and LAPACKE (C interface to LAPACK) provide
# optimized linear algebra operations. This demo shows how to call them from Elle
# using ffi/array to marshal arrays to C.
#
# Key FFI techniques demonstrated:
# - ffi/native: Load shared libraries (libcblas.so.3, liblapacke.so.3)
# - ffi/lookup: Look up C function symbols by name
# - ffi/signature: Create function signatures with return and argument types
# - ffi/array: Create array type descriptors for marshalling
# - ffi/malloc: Allocate C memory
# - ffi/write: Write Elle arrays to C memory
# - ffi/read: Read C arrays back into Elle
# - ffi/call: Call C functions with proper argument marshalling
# - ffi/free: Free C memory
#
# Functions called:
# 1. cblas_ddot: Dot product of two vectors
# 2. cblas_dgemv: Matrix-vector multiply
# 3. cblas_dgemm: Matrix-matrix multiply
# 4. LAPACKE_dgesv: Solve linear system A*X = B
#
# Run with: cargo run -- demos/blas.lisp
# ============================================================================

# Load libraries
(def cblas (ffi/native "libcblas.so.3"))
(def lapacke (ffi/native "liblapacke.so.3"))

# CBLAS enum constants
(def CblasRowMajor 101)
(def CblasNoTrans 111)

# ============================================================================
# Helper: allocate a C double array and write Elle array to it
# ============================================================================
(defn alloc-and-write-doubles (lst)
  "Allocate C memory and write an Elle list of doubles to it"
  (let* ((n (length lst))
         (arr-type (ffi/array :double n))
         (ptr (ffi/malloc (ffi/size arr-type)))
         # Convert list to array using array constructor
         (arr (apply array lst)))
    (ffi/write ptr arr-type arr)
    ptr))

# ============================================================================
# Helper: read n doubles from C memory into an Elle list
# ============================================================================
(defn read-doubles (ptr n)
  "Read n doubles from C memory into an Elle list"
  (let* ((arr-type (ffi/array :double n))
         (result (ffi/read ptr arr-type)))
    result))

# ============================================================================
# Helper: free C memory
# ============================================================================
(defn free-ptr (ptr)
  "Free C memory allocated by ffi/malloc"
  (ffi/free ptr))

# ============================================================================
# 1. DDOT: Dot product of two vectors
# cblas_ddot(n, x, incx, y, incy) -> double
# ============================================================================

(display "=== CBLAS DDOT (Dot Product) ===\n")

(let* ((x (list 1.0 2.0 3.0))
       (y (list 4.0 5.0 6.0))
       (n 3)
       (x-ptr (alloc-and-write-doubles x))
       (y-ptr (alloc-and-write-doubles y)))
  (let* ((ddot-sig (ffi/signature :double @[:int :ptr :int :ptr :int]))
         (ddot-fn (ffi/lookup cblas "cblas_ddot"))
         (result (ffi/call ddot-fn ddot-sig n x-ptr 1 y-ptr 1)))
    (display "x = ")
    (display x)
    (display "\n")
    (display "y = ")
    (display y)
    (display "\n")
    (display "dot(x, y) = ")
    (display result)
    (display "\n")
    (display "expected: 32.0 (1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32)\n")
    (free-ptr x-ptr)
    (free-ptr y-ptr)))

(display "\n")

# ============================================================================
# 2. DGEMV: Matrix-vector multiply
# cblas_dgemv(order, trans, m, n, alpha, a, lda, x, incx, beta, y, incy)
# y := alpha*A*x + beta*y
# ============================================================================

(display "=== CBLAS DGEMV (Matrix-Vector Multiply) ===\n")

(let* ((m 2)
       (n 3)
       (alpha 1.0)
       (beta 0.0)
       # A = [[1, 2, 3],
       #      [4, 5, 6]]  (row-major, 2x3)
       (a (list 1.0 2.0 3.0 4.0 5.0 6.0))
       (x (list 1.0 2.0 3.0))
       (y (list 0.0 0.0))
       (a-ptr (alloc-and-write-doubles a))
       (x-ptr (alloc-and-write-doubles x))
       (y-ptr (alloc-and-write-doubles y)))
  (let* ((dgemv-sig (ffi/signature :void @[:int :int :int :int :double :ptr :int :ptr :int :double :ptr :int]))
         (dgemv-fn (ffi/lookup cblas "cblas_dgemv"))
         (_ (ffi/call dgemv-fn dgemv-sig CblasRowMajor CblasNoTrans m n alpha a-ptr n x-ptr 1 beta y-ptr 1))
         (result (read-doubles y-ptr m)))
    (display "A = [[1, 2, 3],\n")
    (display "     [4, 5, 6]]\n")
    (display "x = ")
    (display x)
    (display "\n")
    (display "y = A*x = ")
    (display result)
    (display "\n")
    (display "expected: [14.0, 32.0]\n")
    (display "  (row 0: 1*1 + 2*2 + 3*3 = 14)\n")
    (display "  (row 1: 4*1 + 5*2 + 6*3 = 32)\n")
    (free-ptr a-ptr)
    (free-ptr x-ptr)
    (free-ptr y-ptr)))

(display "\n")

# ============================================================================
# 3. DGEMM: Matrix-matrix multiply
# cblas_dgemm(order, transa, transb, m, n, k, alpha, a, lda, b, ldb, beta, c, ldc)
# C := alpha*A*B + beta*C
# ============================================================================

(display "=== CBLAS DGEMM (Matrix-Matrix Multiply) ===\n")

(let* ((m 2)
       (n 2)
       (k 3)
       (alpha 1.0)
       (beta 0.0)
       # A = [[1, 2, 3],
       #      [4, 5, 6]]  (2x3)
       (a (list 1.0 2.0 3.0 4.0 5.0 6.0))
       # B = [[1, 2],
       #      [3, 4],
       #      [5, 6]]  (3x2)
       (b (list 1.0 2.0 3.0 4.0 5.0 6.0))
       # C = [[0, 0],
       #      [0, 0]]  (2x2)
       (c (list 0.0 0.0 0.0 0.0))
       (a-ptr (alloc-and-write-doubles a))
       (b-ptr (alloc-and-write-doubles b))
       (c-ptr (alloc-and-write-doubles c)))
  (let* ((dgemm-sig (ffi/signature :void @[:int :int :int :int :int :int :double :ptr :int :ptr :int :double :ptr :int]))
         (dgemm-fn (ffi/lookup cblas "cblas_dgemm"))
         (_ (ffi/call dgemm-fn dgemm-sig CblasRowMajor CblasNoTrans CblasNoTrans m n k alpha a-ptr k b-ptr n beta c-ptr n))
         (result (read-doubles c-ptr (* m n))))
    (display "A = [[1, 2, 3],\n")
    (display "     [4, 5, 6]]\n")
    (display "B = [[1, 2],\n")
    (display "     [3, 4],\n")
    (display "     [5, 6]]\n")
    (display "C = A*B = ")
    (display result)
    (display "\n")
    (display "expected: [22.0, 28.0, 49.0, 64.0]\n")
    (display "  (C[0,0]: 1*1 + 2*3 + 3*5 = 22)\n")
    (display "  (C[0,1]: 1*2 + 2*4 + 3*6 = 28)\n")
    (display "  (C[1,0]: 4*1 + 5*3 + 6*5 = 49)\n")
    (display "  (C[1,1]: 4*2 + 5*4 + 6*6 = 64)\n")
    (free-ptr a-ptr)
    (free-ptr b-ptr)
    (free-ptr c-ptr)))

(display "\n")

# ============================================================================
# 4. DGESV: Solve linear system A*X = B
# LAPACKE_dgesv(matrix_layout, n, nrhs, a, lda, ipiv, b, ldb)
# Note: LAPACKE_dgesv modifies A in place, so we use a copy
# ============================================================================

(display "=== LAPACKE DGESV (Linear System Solve) ===\n")

(let* ((n 2)
       (nrhs 1)
       # A = [[2, 1],
       #      [1, 2]]
       (a (list 2.0 1.0 1.0 2.0))
       # B = [3, 3]  (we want to solve A*X = B)
       (b (list 3.0 3.0))
       (a-ptr (alloc-and-write-doubles a))
       (b-ptr (alloc-and-write-doubles b))
       # ipiv is an integer array for pivot indices
       (ipiv-type (ffi/array :int n))
       (ipiv-ptr (ffi/malloc (ffi/size ipiv-type))))
  (let* ((dgesv-sig (ffi/signature :int @[:int :int :int :ptr :int :ptr :ptr :int]))
         (dgesv-fn (ffi/lookup lapacke "LAPACKE_dgesv"))
         (info (ffi/call dgesv-fn dgesv-sig CblasRowMajor n nrhs a-ptr n ipiv-ptr b-ptr nrhs))
         (result (read-doubles b-ptr n)))
    (display "Solve A*X = B where:\n")
    (display "A = [[2, 1],\n")
    (display "     [1, 2]]\n")
    (display "B = [3, 3]\n")
    (display "X = ")
    (display result)
    (display "\n")
    (display "expected: [1.0, 1.0] (A*[1,1] = [2+1, 1+2] = [3,3])\n")
    (display "info = ")
    (display info)
    (display " (0 = success)\n")
    (free-ptr a-ptr)
    (free-ptr b-ptr)
    (free-ptr ipiv-ptr)))

(display "\n")
(display "=== All BLAS/LAPACK demos completed successfully ===\n")
