# Tests for the environment primitive

(import-file "tests/elle/assert.lisp")

(assert-true (struct? (environment)) "environment returns struct")

(assert-true
  (not (nil? (get (environment) :+)))
  "environment contains primitives")

(assert-eq
  (begin (def my-val 42) (get (environment) :my-val))
  42
  "environment contains user defined global")

(assert-eq
  (begin (var x 1) (set x 2) (get (environment) :x))
  2
  "environment reflects mutation")

(assert-true
  (struct? (vm/query "environment" nil))
  "environment via vm query")

(assert-true
  (nil? (get (environment) :__nonexistent_symbol_42__))
  "environment excludes undefined")
