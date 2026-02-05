#!/usr/bin/elle
;; Module System Example - Comprehensive Demonstration
;; This example showcases all features of Elle's module system

(begin
  (display "=== Elle Module System ===")
  (newline)
  (newline)

  ;; Part 1: Built-in String Module
  (display "Part 1: Built-in String Module")
  (newline)
  (display "---")
  (newline)

  (display "String functions:")
  (newline)
  (display "  string-length(\"Hello\") = ")
  (display (string-length "Hello"))
  (newline)

  (display "  string-upcase(\"hello\") = ")
  (display (string-upcase "hello"))
  (newline)

  (display "  string-downcase(\"WORLD\") = ")
  (display (string-downcase "WORLD"))
  (newline)

  (display "  substring(\"Elle\", 0, 2) = ")
  (display (substring "Elle" 0 2))
  (newline)
  (newline)

  ;; Part 2: Built-in List Module
  (display "Part 2: Built-in List Module")
  (newline)
  (display "---")
  (newline)

  (define test-list (list 1 2 3 4 5))
  (display "Test list: ")
  (display test-list)
  (newline)

  (display "  length = ")
  (display (length test-list))
  (newline)

  (display "  reverse = ")
  (display (reverse test-list))
  (newline)

  (display "  append with (6 7) = ")
  (display (append test-list (list 6 7)))
  (newline)

  (display "  first = ")
  (display (first test-list))
  (newline)

  (display "  rest = ")
  (display (rest test-list))
  (newline)

  (display "  nth(1) (second element) = ")
  (display (nth 1 test-list))
  (newline)
  (newline)

  ;; Part 3: Built-in Math Module
  (display "Part 3: Built-in Math Module")
  (newline)
  (display "---")
  (newline)

  (display "Math functions:")
  (newline)
  (display "  sqrt(16) = ")
  (display (sqrt 16))
  (newline)

  (display "  pow(2, 3) = ")
  (display (pow 2 3))
  (newline)

  (display "  sin(0) = ")
  (display (sin 0))
  (newline)

  (display "  cos(0) = ")
  (display (cos 0))
  (newline)

  (display "  floor(3.7) = ")
  (display (floor 3.7))
  (newline)

  (display "  ceil(3.2) = ")
  (display (ceil 3.2))
  (newline)

  (display "  pi = ")
  (display pi)
  (newline)

  (display "  e = ")
  (display e)
  (newline)
  (newline)

   ;; Part 4: Arithmetic and Comparisons
   (display "Part 4: Arithmetic and Comparisons")
   (newline)
   (display "---")
   (newline)

   (define a 10)
   (define b 3)
   (display "Arithmetic on numbers:")
   (newline)
   (display "  10 + 3 = ")
   (display (+ a b))
   (newline)

   (display "  10 - 3 = ")
   (display (- a b))
   (newline)

   (display "  10 * 3 = ")
   (display (* a b))
   (newline)

   (display "  10 / 3 = ")
   (display (/ a b))
   (newline)

   (display "  10 mod 3 = ")
   (display (mod a b))
   (newline)
   (newline)

   ;; Part 5: Working with Strings in Lists
   (display "Part 5: String Module with Lists")
   (newline)
   (display "---")
   (newline)

   (define str1 "hello")
   (define str2 "world")
   (display "String operations on individual values:")
   (newline)
   (display "  string-upcase(\"hello\") = ")
   (display (string-upcase str1))
   (newline)

   (display "  string-length(\"world\") = ")
   (display (string-length str2))
   (newline)

   (display "  string-append(\"hello\", \", \", \"world\") = ")
   (display (string-append str1 ", " str2))
   (newline)
   (newline)

   ;; Part 6: Math Module Operations
   (display "Part 6: Advanced Math Operations")
   (newline)
   (display "---")
   (newline)

   (display "Power calculations:")
   (newline)
   (display "  2^2 = ")
   (display (pow 2 2))
   (newline)

   (display "  3^3 = ")
   (display (pow 3 3))
   (newline)

   (display "  4^2 = ")
   (display (pow 4 2))
   (newline)

   (display "Square root of 144 = ")
   (display (sqrt 144))
   (newline)

   (display "Trigonometric functions (radians):")
   (newline)
   (display "  sin(pi/2) ≈ ")
   (display (sin (/ 3.14159 2)))
   (newline)
   (newline)

  ;; Part 7: Module Path Management
  (display "Part 7: Module System Capabilities")
  (newline)
  (display "---")
  (newline)

  (add-module-path "test-modules")
  (display "Added 'test-modules' to module search path")
  (newline)

  (display "Can import external modules with (import-file path)")
  (newline)
  (newline)

  ;; Part 8: Type Checking and Validation
  (display "Part 8: Type Checking")
  (newline)
  (display "---")
  (newline)

  (display "Type predicates:")
  (newline)
  (display "  string?(\"hello\") = ")
  (display (string? "hello"))
  (newline)

  (display "  number?(42) = ")
  (display (number? 42))
  (newline)

  (display "  pair?((1 2 3)) = ")
  (display (pair? (list 1 2 3)))
  (newline)

  (display "  nil?(()) = ")
  (display (nil? (list)))
  (newline)
  (newline)

  ;; Part 9: String Manipulation
  (display "Part 9: String Module Operations")
  (newline)
  (display "---")
  (newline)

  (display "String concatenation:")
  (newline)
  (display "  \"Hello\" + \" \" + \"World\" = ")
  (display (string-append "Hello" " " "World"))
  (newline)

  (display "Character operations:")
  (newline)
  (display "  char-at(\"Elle\", 0) = ")
  (display (char-at "Elle" 0))
  (newline)

  (display "  string-index(\"Elle\", \"l\") = ")
  (display (string-index "Elle" "l"))
  (newline)
  (newline)

  ;; Part 10: List Utilities
  (display "Part 10: List Module Operations")
  (newline)
  (display "---")
  (newline)

   (define big-list (list 10 20 30 40 50))
   (display "List operations:")
   (newline)
   (display "  last([10,20,30,40,50]) = ")
   (display (last big-list))
   (newline)

   (display "  take([10,20,30,40,50], 3) = ")
   (display (take 3 big-list))
   (newline)

   (display "  drop([10,20,30,40,50], 2) = ")
   (display (drop 2 big-list))
   (newline)
   (newline)

  ;; Part 11: Conditional Logic
  (display "Part 11: Using Modules in Conditionals")
  (newline)
  (display "---")
  (newline)

  (define test-str "Module")
  (display "If string-length(\"")
  (display test-str)
  (display "\") > 5: ")
  (display (if (> (string-length test-str) 5) "true" "false"))
  (newline)

  (define test-list2 (list 1 2 3))
  (display "If length([1,2,3]) < 5: ")
  (display (if (< (length test-list2) 5) "true" "false"))
  (newline)
  (newline)

  ;; Part 12: Error Handling
  (display "Part 12: Module Functions are Safe")
  (newline)
  (display "---")
  (newline)

  (display "All module functions include proper error handling")
  (newline)
  (display "Type mismatches and bounds errors are caught")
  (newline)
  (newline)

  ;; Summary
  (display "=== Module System Summary ===")
  (newline)
  (newline)

  (display "Built-in Modules:")
  (newline)
  (display "1. String Module:")
  (newline)
  (display "   - string-length, string-append, string-upcase, string-downcase")
  (newline)
  (display "   - substring, string-index, char-at")
  (newline)
  (newline)

  (display "2. List Module:")
  (newline)
  (display "   - length, reverse, append, cons, first, rest")
  (newline)
  (display "   - map, filter, fold, take, drop, nth, last")
  (newline)
  (newline)

  (display "3. Math Module:")
  (newline)
  (display "   - +, -, *, /, mod, sqrt, pow, sin, cos, tan")
  (newline)
  (display "   - floor, ceil, round, log, exp, pi, e")
  (newline)
  (newline)

   (display "Module Features:")
   (newline)
   (display "- Built-in String, List, and Math modules")
   (newline)
   (display "- Import external modules with (import-file path)")
   (newline)
   (display "- Add search paths with (add-module-path path)")
   (newline)
   (display "- Compose functions from different modules")
   (newline)
   (display "- Safe type checking and error handling")
   (newline)
   (newline)

  (display "=== Module System Example Complete ===")
  (newline))
