# BLAS/LAPACK FFI Demo

## What This Demo Does

This demo demonstrates calling optimized numerical routines from C libraries via Elle's FFI (Foreign Function Interface). It shows how to:
- Load shared libraries (`libcblas.so.3`, `liblapacke.so.3`)
- Look up C function symbols
- Create function signatures with proper types
- Marshal Elle arrays to/from C memory
- Call C functions with correct argument passing

The demo exercises four key linear algebra operations:
1. **DDOT** — Dot product of two vectors
2. **DGEMV** — Matrix-vector multiplication
3. **DGEMM** — Matrix-matrix multiplication
4. **DGESV** — Solve a linear system

## How It Works

### Loading Libraries

```janet
(def cblas (ffi/native "libcblas.so.3"))
(def lapacke (ffi/native "liblapacke.so.3"))
```

`ffi/native` loads a shared library and returns a handle for looking up symbols.

### Helper Functions

**`alloc-and-write-doubles`** — Allocate C memory and write an Elle array to it
```janet
(defn alloc-and-write-doubles (lst)
  (let* [n (length lst)
         arr-type (ffi/array :double n)
         ptr (ffi/malloc (ffi/size arr-type))
         arr (apply array lst)]
    (ffi/write ptr arr-type arr)
    ptr))
```

This:
1. Creates an array type descriptor for N doubles
2. Allocates C memory of that size
3. Converts the Elle list to an array
4. Writes the array to C memory
5. Returns the C pointer

**`read-doubles`** — Read N doubles from C memory into an Elle array
```janet
(defn read-doubles (ptr n)
  (let* [arr-type (ffi/array :double n)
         result (ffi/read ptr arr-type)]
    result))
```

**`free-ptr`** — Free C memory
```janet
(defn free-ptr (ptr)
  (ffi/free ptr))
```

### CBLAS Constants

```janet
(def CblasRowMajor 101)
(def CblasNoTrans 111)
```

These are CBLAS enum values that control matrix layout and transposition.

### Operation 1: DDOT (Dot Product)

```janet
(let* [x (list 1.0 2.0 3.0)
       y (list 4.0 5.0 6.0)
       n 3
       x-ptr (alloc-and-write-doubles x)
       y-ptr (alloc-and-write-doubles y)]
  (let* [ddot-sig (ffi/signature :double @[:int :ptr :int :ptr :int])
         ddot-fn (ffi/lookup cblas "cblas_ddot")
         result (ffi/call ddot-fn ddot-sig n x-ptr 1 y-ptr 1)]
    ...))
```

The signature specifies:
- Return type: `:double`
- Arguments: `[:int :ptr :int :ptr :int]` (n, x_ptr, incx, y_ptr, incy)

The call computes: `x·y = 1*4 + 2*5 + 3*6 = 32`

### Operation 2: DGEMV (Matrix-Vector Multiply)

```janet
(let* [m 2  n 3  alpha 1.0  beta 0.0
       a (list 1.0 2.0 3.0 4.0 5.0 6.0)  # 2×3 matrix
       x (list 1.0 2.0 3.0)               # 3-element vector
       y (list 0.0 0.0)                   # 2-element result
       ...]
  (let* [dgemv-sig (ffi/signature :void @[:int :int :int :int :double :ptr :int :ptr :int :double :ptr :int])
         dgemv-fn (ffi/lookup cblas "cblas_dgemv")
         _ (ffi/call dgemv-fn dgemv-sig CblasRowMajor CblasNoTrans m n alpha a-ptr n x-ptr 1 beta y-ptr 1)
         result (read-doubles y-ptr m)]
    ...))
```

Computes: `y = A*x`
- A = [[1, 2, 3], [4, 5, 6]]
- x = [1, 2, 3]
- y = [1*1 + 2*2 + 3*3, 4*1 + 5*2 + 6*3] = [14, 32]

### Operation 3: DGEMM (Matrix-Matrix Multiply)

```janet
(let* [m 2  n 2  k 3  alpha 1.0  beta 0.0
       a (list 1.0 2.0 3.0 4.0 5.0 6.0)  # 2×3
       b (list 1.0 2.0 3.0 4.0 5.0 6.0)  # 3×2
       c (list 0.0 0.0 0.0 0.0)          # 2×2 result
       ...]
  (let* [dgemm-sig (ffi/signature :void @[:int :int :int :int :int :int :double :ptr :int :ptr :int :double :ptr :int])
         dgemm-fn (ffi/lookup cblas "cblas_dgemm")
         _ (ffi/call dgemm-fn dgemm-sig CblasRowMajor CblasNoTrans CblasNoTrans m n k alpha a-ptr k b-ptr n beta c-ptr n)
         result (read-doubles c-ptr (* m n))]
    ...))
```

Computes: `C = A*B`
- A = [[1, 2, 3], [4, 5, 6]] (2×3)
- B = [[1, 2], [3, 4], [5, 6]] (3×2)
- C = [[22, 28], [49, 64]] (2×2)

### Operation 4: DGESV (Linear System Solve)

```janet
(let* [n 2  nrhs 1
       a (list 2.0 1.0 1.0 2.0)  # 2×2 matrix
       b (list 3.0 3.0)          # 2-element RHS
       a-ptr (alloc-and-write-doubles a)
       b-ptr (alloc-and-write-doubles b)
       ipiv-type (ffi/array :int n)
       ipiv-ptr (ffi/malloc (ffi/size ipiv-type))]
  (let* [dgesv-sig (ffi/signature :int @[:int :int :int :ptr :int :ptr :ptr :int])
         dgesv-fn (ffi/lookup lapacke "LAPACKE_dgesv")
         info (ffi/call dgesv-fn dgesv-sig CblasRowMajor n nrhs a-ptr n ipiv-ptr b-ptr nrhs)
         result (read-doubles b-ptr n)]
    ...))
```

Solves: `A*X = B`
- A = [[2, 1], [1, 2]]
- B = [3, 3]
- X = [1, 1] (because 2*1 + 1*1 = 3 and 1*1 + 2*1 = 3)

The `info` return value is 0 for success.

## Sample Output

```
=== CBLAS DDOT (Dot Product) ===
x = (1 2 3)
y = (4 5 6)
dot(x, y) = 32
expected: 32.0 (1*4 + 2*5 + 3*6 = 4 + 10 + 18 = 32)

=== CBLAS DGEMV (Matrix-Vector Multiply) ===
A = [[1, 2, 3],
     [4, 5, 6]]
x = (1 2 3)
y = A*x = @[14 32]
expected: [14.0, 32.0]
  (row 0: 1*1 + 2*2 + 3*3 = 14)
  (row 1: 4*1 + 5*2 + 6*3 = 32)

=== CBLAS DGEMM (Matrix-Matrix Multiply) ===
A = [[1, 2, 3],
     [4, 5, 6]]
B = [[1, 2],
     [3, 4],
     [5, 6]]
C = A*B = @[22 28 49 64]
expected: [22.0, 28.0, 49.0, 64.0]
  (C[0,0]: 1*1 + 2*3 + 3*5 = 22)
  (C[0,1]: 1*2 + 2*4 + 3*6 = 28)
  (C[1,0]: 4*1 + 5*3 + 6*5 = 49)
  (C[1,1]: 4*2 + 5*4 + 6*6 = 64)

=== LAPACKE DGESV (Linear System Solve) ===
Solve A*X = B where:
A = [[2, 1],
     [1, 2]]
B = [3, 3]
X = @[1 1]
expected: [1.0, 1.0] (A*[1,1] = [2+1, 1+2] = [3,3])
info = 0 (0 = success)

=== All BLAS/LAPACK demos completed successfully ===
```

## Elle Idioms Used

- **`defn`** — Function definition
- **`let*`** — Sequential bindings
- **`apply`** — Call a function with arguments spread from a list
- **FFI primitives:**
  - `ffi/native` — Load a shared library
  - `ffi/lookup` — Look up a C function symbol
  - `ffi/signature` — Create a function signature
  - `ffi/array` — Create an array type descriptor
  - `ffi/malloc` / `ffi/free` — Allocate/free C memory
  - `ffi/write` / `ffi/read` — Marshal data to/from C
  - `ffi/call` — Call a C function

## Why This Demo?

BLAS and LAPACK are industry-standard libraries for numerical computing. This demo shows:
1. **FFI capability** — Elle can call optimized C code
2. **Memory marshalling** — Proper handling of C pointers and arrays
3. **Type safety** — Function signatures ensure correct calling conventions
4. **Performance** — Leveraging optimized libraries for numerical work

## Running the Demo

```bash
cargo run --release -- demos/blas.lisp
```

This requires `libcblas.so.3` and `liblapacke.so.3` to be installed. On Debian/Ubuntu:
```bash
sudo apt-get install libblas-dev liblapack-dev
```

On Fedora/RHEL:
```bash
sudo dnf install blas-devel lapack-devel
```

## Further Reading

- [CBLAS documentation](http://www.netlib.org/blas/blast-forum/cblas.pdf)
- [LAPACKE documentation](http://www.netlib.org/lapack/lapacke.html)
- Elle FFI guide (in docs/)
