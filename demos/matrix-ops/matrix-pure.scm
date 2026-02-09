;; Matrix Operations - Pure Lisp Implementation
;; Chez Scheme version
;;
;; This demo tests:
;; - Vector/list operations for dense matrix representation
;; - Numeric computation (multiply, transpose, decomposition)
;; - Algorithm clarity and performance in pure functional code
;;
;; Matrix representation: vector of vectors (row-major)
;; Each row is a vector of numbers

;; ============================================================================
;; Matrix Creation and Access
;; ============================================================================

;; Create a zero matrix of given dimensions
(define (make-matrix rows cols)
  (make-vector rows
    (make-vector cols 0.0)))

;; Create an identity matrix
(define (identity n)
  (let ((m (make-matrix n n)))
    (do ((i 0 (+ i 1)))
      ((= i n) m)
      (vector-set! (vector-ref m i) i 1.0))))

;; Get element at (i, j)
(define (matrix-ref m i j)
  (vector-ref (vector-ref m i) j))

;; Set element at (i, j)
(define (matrix-set! m i j val)
  (vector-set! (vector-ref m i) j val))

;; Get dimensions
(define (matrix-rows m)
  (vector-length m))

(define (matrix-cols m)
  (if (> (vector-length m) 0)
    (vector-length (vector-ref m 0))
    0))

;; ============================================================================
;; Matrix Operations
;; ============================================================================

;; Matrix transpose
(define (matrix-transpose m)
  (let* ((rows (matrix-rows m))
         (cols (matrix-cols m))
         (result (make-matrix cols rows)))
    (do ((i 0 (+ i 1)))
      ((= i rows) result)
      (do ((j 0 (+ j 1)))
        ((= j cols))
        (matrix-set! result j i (matrix-ref m i j))))))

;; Matrix multiplication: (m1: a x b) * (m2: b x c) = (result: a x c)
(define (matrix-multiply m1 m2)
  (let* ((a (matrix-rows m1))
         (b (matrix-cols m1))
         (c (matrix-cols m2))
         (result (make-matrix a c)))
    (do ((i 0 (+ i 1)))
      ((= i a) result)
      (do ((j 0 (+ j 1)))
        ((= j c))
        ;; Compute dot product of row i and column j
        (let ((sum 0.0))
          (do ((k 0 (+ k 1)))
            ((= k b))
            (set! sum (+ sum
              (* (matrix-ref m1 i k)
                 (matrix-ref m2 k j)))))
          (matrix-set! result i j sum))))))

;; Matrix addition
(define (matrix-add m1 m2)
  (let* ((rows (matrix-rows m1))
         (cols (matrix-cols m1))
         (result (make-matrix rows cols)))
    (do ((i 0 (+ i 1)))
      ((= i rows) result)
      (do ((j 0 (+ j 1)))
        ((= j cols))
        (matrix-set! result i j
          (+ (matrix-ref m1 i j)
             (matrix-ref m2 i j)))))))

;; Scalar multiplication
(define (matrix-scale m scalar)
  (let* ((rows (matrix-rows m))
         (cols (matrix-cols m))
         (result (make-matrix rows cols)))
    (do ((i 0 (+ i 1)))
      ((= i rows) result)
      (do ((j 0 (+ j 1)))
        ((= j cols))
        (matrix-set! result i j
          (* scalar (matrix-ref m i j)))))))

;; Frobenius norm (sum of squared elements)
(define (matrix-norm m)
  (let ((rows (matrix-rows m))
        (cols (matrix-cols m))
        (sum 0.0))
    (do ((i 0 (+ i 1)))
      ((= i rows) (sqrt sum))
      (do ((j 0 (+ j 1)))
        ((= j cols))
        (let ((val (matrix-ref m i j)))
          (set! sum (+ sum (* val val))))))))

;; ============================================================================
;; LU Decomposition with Partial Pivoting
;; Decomposes M into L*U where L is lower triangular, U is upper triangular
;; ============================================================================

(define (lu-decomposition m)
  (let* ((n (matrix-rows m))
         ;; Create working copies
         (a (make-matrix n n)))
    ;; Copy input matrix
    (do ((i 0 (+ i 1)))
      ((= i n))
      (do ((j 0 (+ j 1)))
        ((= j n))
        (matrix-set! a i j (matrix-ref m i j))))
    
    ;; Perform LU decomposition with partial pivoting
    (do ((k 0 (+ k 1)))
      ((= k n) a)
      
      ;; Find pivot row
      (let ((max-val 0.0)
            (pivot-row k))
        (do ((i k (+ i 1)))
          ((= i n))
          (if (> (abs (matrix-ref a i k)) max-val)
            (begin
              (set! max-val (abs (matrix-ref a i k)))
              (set! pivot-row i))))
        
        ;; Swap rows if needed
        (if (not (= pivot-row k))
          (let ((temp (vector-ref a k)))
            (vector-set! a k (vector-ref a pivot-row))
            (vector-set! a pivot-row temp)))
        
        ;; Skip if pivot is too small
        (if (> (abs (matrix-ref a k k)) 1e-10)
          (begin
            ;; Eliminate below pivot
            (do ((i (+ k 1) (+ i 1)))
              ((= i n))
              ;; Compute multiplier and store in lower triangle
              (let ((factor (/ (matrix-ref a i k) (matrix-ref a k k))))
                (matrix-set! a i k factor)
                ;; Eliminate elements in upper triangle
                (do ((j (+ k 1) (+ j 1)))
                  ((= j n))
                  (matrix-set! a i j
                    (- (matrix-ref a i j)
                       (* factor (matrix-ref a k j)))))))))))
    
    a))

;; ============================================================================
;; Benchmarks
;; ============================================================================

(define (benchmark-multiply size)
  (display "Matrix multiply (")
  (display size)
  (display "x")
  (display size)
  (display "): ")
  (let ((m1 (make-matrix size size))
        (m2 (make-matrix size size)))
    ;; Fill with random-ish values
    (do ((i 0 (+ i 1)))
      ((= i size))
      (do ((j 0 (+ j 1)))
        ((= j size))
        (matrix-set! m1 i j (+ 0.1 (/ (+ i j) size)))
        (matrix-set! m2 i j (+ 0.1 (/ (+ i j) size)))))
    
    (let ((start (current-time 'time-utc)))
      (let ((result (matrix-multiply m1 m2)))
        (let ((end (current-time 'time-utc)))
          (let ((elapsed (- (time-second end) (time-second start))))
            (display "done in ~")
            (display (+ (* elapsed 1000)
              (/ (- (time-nanosecond end) (time-nanosecond start)) 1e6)))
            (display "ms, norm=")
            (display (matrix-norm result))
            (newline)))))))

(define (benchmark-transpose size)
  (display "Matrix transpose (")
  (display size)
  (display "x")
  (display size)
  (display "): ")
  (let ((m (make-matrix size size)))
    ;; Fill with values
    (do ((i 0 (+ i 1)))
      ((= i size))
      (do ((j 0 (+ j 1)))
        ((= j size))
        (matrix-set! m i j (+ 0.1 (/ (+ i j) size)))))
    
    (let ((start (current-time 'time-utc)))
      (let ((result (matrix-transpose m)))
        (let ((end (current-time 'time-utc)))
          (let ((elapsed (- (time-second end) (time-second start))))
            (display "done in ~")
            (display (+ (* elapsed 1000)
              (/ (- (time-nanosecond end) (time-nanosecond start)) 1e6)))
            (display "ms")
            (newline)))))))

(define (benchmark-lu size)
  (display "LU decomposition (")
  (display size)
  (display "x")
  (display size)
  (display "): ")
  (let ((m (make-matrix size size)))
    ;; Create a diagonally dominant matrix (guaranteed non-singular)
    (do ((i 0 (+ i 1)))
      ((= i size))
      (do ((j 0 (+ j 1)))
        ((= j size))
        (if (= i j)
          (matrix-set! m i j (+ 10.0 (/ (+ i j) size)))
          (matrix-set! m i j (/ (+ i j) size)))))
    
    (let ((start (current-time 'time-utc)))
      (let ((result (lu-decomposition m)))
        (let ((end (current-time 'time-utc)))
          (let ((elapsed (- (time-second end) (time-second start))))
            (display "done in ~")
            (display (+ (* elapsed 1000)
              (/ (- (time-nanosecond end) (time-nanosecond start)) 1e6)))
            (display "ms")
            (newline)))))))

;; ============================================================================
;; Main
;; ============================================================================

(display "=== Matrix Operations (Pure Chez Scheme) ===")
(newline)
(newline)

;; Small test
(display "Small matrix (16x16):")
(newline)
(benchmark-multiply 16)
(benchmark-transpose 16)
(benchmark-lu 16)
(newline)

;; Medium test
(display "Medium matrix (64x64):")
(newline)
(benchmark-multiply 64)
(benchmark-transpose 64)
(benchmark-lu 64)
(newline)

;; Larger test
(display "Large matrix (256x256):")
(newline)
(benchmark-multiply 256)
(benchmark-transpose 256)
(benchmark-lu 256)
(newline)

(display "=== Complete ===")
(newline)
