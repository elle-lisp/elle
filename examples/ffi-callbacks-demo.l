;;; FFI Phase 3 Example: C Function Pointer Callbacks
;;;
;;; This example demonstrates callback functionality:
;;; - Creating Elle closures as C function pointers
;;; - Registering callbacks with Elle's callback registry
;;; - Passing callbacks to C functions
;;; - Cleaning up callbacks
;;;
;;; Callbacks enable bidirectional FFI communication where Elle code
;;; can pass functions to C code that call them back.

(print "FFI Phase 3: C Function Pointer Callbacks")
(print "==========================================")

; Example 1: Creating a simple callback
(print "1. Creating an Elle closure as a C callback...")

; Define a closure that will be called from C code
; This is a simple comparator function for sorting
(define compare-ints
  (lambda (a b)
    ; Compare two integers
    ; Return: negative if a < b, zero if equal, positive if a > b
    (cond
      ((< a b) -1)
      ((> a b) 1)
      (else 0))))

(print "   Created closure: compare-ints")

; Example 2: Registering the callback
(print "2. Registering callback with C runtime...")

; In actual use, we would call:
; (make-c-callback compare-ints (:int :int) :int)
;
; This would:
; - Take the Elle closure (compare-ints)
; - Register it in the callback registry
; - Return a callback ID that can be used in C calls
; - Type signature: takes two ints, returns int
;
; Note: Actual C invocation requires integration with C code

(print "   Callback registration would create a C function pointer")

; Example 3: Using callbacks with C functions
(print "3. Passing callbacks to C functions...")

; In actual use with libc's qsort:
; (define sorted-array
;   (ffi-call "libc" "qsort"
;     array-ptr           ; pointer to array
;     array-length        ; number of elements
;     element-size        ; size of each element
;     compare-callback))  ; our registered callback
;
; The C qsort function would then call our Elle closure
; for each comparison during the sort

(print "   Would call C qsort with Elle comparator")

; Example 4: Multiple callbacks
(print "4. Working with multiple callbacks...")

; Define different callback types
(define compare-strings
  (lambda (s1 s2)
    ; String comparison
    ; Return: negative if s1 < s2, zero if equal, positive if s1 > s2
    (cond
      ((string<? s1 s2) -1)
      ((string>? s1 s2) 1)
      (else 0))))

(define filter-positive
  (lambda (value)
    ; Filter callback: return true if value is positive
    (> value 0)))

(print "   Created multiple callback closures")
(print "   - compare-strings: for string comparison")
(print "   - filter-positive: for filtering arrays")

; Example 5: Callback lifecycle
(print "5. Callback lifecycle management...")

; When done with a callback:
; (free-callback callback-id)
;
; This would:
; - Unregister the callback from the registry
; - Drop the reference to the Elle closure
; - Free any associated memory
; - Allow the GC to collect the closure if not referenced elsewhere

(print "   Callbacks are registered until explicitly freed")
(print "   (free-callback callback-id) cleans up resources")

; Example 6: Error handling
(print "6. Error handling with callbacks...")

; If an Elle closure raises an exception while being called from C,
; the callback system will:
; - Catch the exception
; - Convert it to a C-compatible error value
; - Return safely to C code
; - Allow proper cleanup and recovery

(print "   Exceptions in Elle closures are caught and converted to C errors")

; Example 7: Type safety
(print "7. Type-safe callbacks...")

; The callback system validates:
; - Argument count matches expected signature
; - Argument types can be marshaled correctly
; - Return type can be converted back to C type
; - Prevents calling with mismatched signatures

(print "   Callbacks validate argument/return types at registration time")

(print "==========================================")
(print "FFI Phase 3: Callback Example Complete!")
(print "")
(print "Key Features:")
(print "- Elle closures can be used as C function pointers")
(print "- Bidirectional FFI communication")
(print "- Type-safe callback registration")
(print "- Automatic exception handling")
(print "- Callback lifecycle management")
(print "")
(print "Next: Phase 4 will add advanced features like")
(print "  - Callback pools for high-frequency calls")
(print "  - Performance optimization for hot callbacks")
(print "  - Cross-thread callback safety")
