;; Matrix Operations - Pure Common Lisp Implementation (SBCL)
;;
;; Tests: numeric computation, algorithm clarity, performance

(defun make-matrix (rows cols &optional (initial-value 0.0d0))
  "Create a rows x cols matrix"
  (make-array (list rows cols) :initial-element (float initial-value 1.0d0)))

(defun matrix-rows (m)
  "Get number of rows"
  (array-dimension m 0))

(defun matrix-cols (m)
  "Get number of columns"
  (array-dimension m 1))

;; Matrix transpose
(defun matrix-transpose (m)
  "Return the transpose of matrix m"
  (let* ((rows (matrix-rows m))
         (cols (matrix-cols m))
         (result (make-matrix cols rows 0.0d0)))
    (dotimes (i rows result)
      (dotimes (j cols)
        (setf (aref result j i) (aref m i j))))))

;; Matrix multiply: (m1: a x b) * (m2: b x c) = (result: a x c)
(defun matrix-multiply (m1 m2)
  "Multiply matrices m1 and m2"
  (let* ((a (matrix-rows m1))
         (b (matrix-cols m1))
         (c (matrix-cols m2))
         (result (make-matrix a c 0.0d0)))
    (dotimes (i a result)
      (dotimes (j c)
        (let ((sum 0.0d0))
          (dotimes (k b)
            (incf sum (* (aref m1 i k) (aref m2 k j))))
          (setf (aref result i j) sum))))))

;; Matrix addition
(defun matrix-add (m1 m2)
  "Add two matrices"
  (let* ((rows (matrix-rows m1))
         (cols (matrix-cols m1))
         (result (make-matrix rows cols 0.0d0)))
    (dotimes (i rows result)
      (dotimes (j cols)
        (setf (aref result i j) (+ (aref m1 i j) (aref m2 i j)))))))

;; Scalar multiply
(defun matrix-scale (m scalar)
  "Multiply matrix by scalar"
  (let* ((rows (matrix-rows m))
         (cols (matrix-cols m))
         (result (make-matrix rows cols 0.0d0)))
    (dotimes (i rows result)
      (dotimes (j cols)
        (setf (aref result i j) (* scalar (aref m i j)))))))

;; Frobenius norm
(defun matrix-norm (m)
  "Compute Frobenius norm (sqrt of sum of squared elements)"
  (let ((rows (matrix-rows m))
        (cols (matrix-cols m))
        (sum 0.0d0))
    (dotimes (i rows (sqrt sum))
      (dotimes (j cols)
        (let ((val (aref m i j)))
          (incf sum (* val val)))))))

;; LU decomposition
(defun lu-decomposition (m)
  "Perform LU decomposition with partial pivoting"
  (let* ((n (matrix-rows m))
         (a (make-matrix n n 0.0d0)))
    ;; Copy input
    (dotimes (i n)
      (dotimes (j n)
        (setf (aref a i j) (aref m i j))))
    
    ;; Decompose
    (dotimes (k n a)
      (let ((max-val 0.0d0) (pivot-row k))
        ;; Find pivot
        (dotimes (i (- n k))
          (let ((row (+ k i)))
            (when (> (abs (aref a row k)) max-val)
              (setf max-val (abs (aref a row k)) pivot-row row))))
        
        ;; Swap rows
        (when (not (= pivot-row k))
          (dotimes (j n)
            (rotatef (aref a k j) (aref a pivot-row j))))
        
        ;; Eliminate
        (when (> (abs (aref a k k)) 1.0d-10)
          (dotimes (i (- n k 1))
            (let ((row (+ k i 1)))
              (let ((factor (/ (aref a row k) (aref a k k))))
                (setf (aref a row k) factor)
                (dotimes (j (- n k 1))
                  (let ((col (+ k j 1)))
                    (decf (aref a row col) (* factor (aref a k col)))))))))))))

;; Benchmarks
(defun benchmark-multiply (size)
  (format t "Matrix multiply (~Dx~D): " size size)
  (let ((m1 (make-matrix size size 0.0d0))
        (m2 (make-matrix size size 0.0d0)))
    ;; Fill
    (dotimes (i size)
      (dotimes (j size)
        (setf (aref m1 i j) (float (+ 0.1d0 (/ (+ i j) size)) 1.0d0))
              (aref m2 i j) (float (+ 0.1d0 (/ (+ i j) size)) 1.0d0))))
    
    (let ((start (get-internal-real-time)))
      (let ((result (matrix-multiply m1 m2)))
        (let ((end (get-internal-real-time)))
          (let ((elapsed (/ (- end start) internal-time-units-per-second)))
            (format t "done in ~Fms, norm=~F~%" 
              (* elapsed 1000.0d0)
              (matrix-norm result))))))))

(defun benchmark-transpose (size)
  (format t "Matrix transpose (~Dx~D): " size size)
  (let ((m (make-matrix size size 0.0d0)))
    (dotimes (i size)
      (dotimes (j size)
        (setf (aref m i j) (float (+ 0.1d0 (/ (+ i j) size)) 1.0d0))))
    
    (let ((start (get-internal-real-time)))
      (let ((result (matrix-transpose m)))
        (let ((end (get-internal-real-time)))
          (let ((elapsed (/ (- end start) internal-time-units-per-second)))
            (format t "done in ~Fms~%" (* elapsed 1000.0d0))))))))

(defun benchmark-lu (size)
  (format t "LU decomposition (~Dx~D): " size size)
  (let ((m (make-matrix size size 0.0d0)))
    ;; Diagonally dominant
    (dotimes (i size)
      (dotimes (j size)
        (if (= i j)
          (setf (aref m i j) (float (+ 10.0d0 (/ (+ i j) size)) 1.0d0))
          (setf (aref m i j) (float (/ (+ i j) size) 1.0d0)))))
    
    (let ((start (get-internal-real-time)))
      (let ((result (lu-decomposition m)))
        (let ((end (get-internal-real-time)))
          (let ((elapsed (/ (- end start) internal-time-units-per-second)))
            (format t "done in ~Fms~%" (* elapsed 1000.0d0))))))))

;; Main
(format t "=== Matrix Operations (Pure SBCL) ===~%~%")

(format t "Small matrix (16x16):~%")
(benchmark-multiply 16)
(benchmark-transpose 16)
(benchmark-lu 16)
(terpri)

(format t "Medium matrix (64x64):~%")
(benchmark-multiply 64)
(benchmark-transpose 64)
(benchmark-lu 64)
(terpri)

(format t "Large matrix (256x256):~%")
(benchmark-multiply 256)
(benchmark-transpose 256)
(benchmark-lu 256)
(terpri)

(format t "=== Complete ===~%")
