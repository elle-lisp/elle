# Integration tests for the new Syntax → HIR → LIR compilation pipeline
#
# Migrated from tests/integration/new_pipeline.rs
# Tests that use eval_source() with value comparisons or .is_err()
# Tests that use compiles()/compile() stay in Rust

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============ Loop Tests ============

# test_each_simple
(assert-eq (let ((sum 0)) (each x '(1 2 3) (assign sum (+ sum x))) sum) 6
  "each simple")

# test_each_with_in
(assert-eq (let ((sum 0)) (each x in '(1 2 3) (assign sum (+ sum x))) sum) 6
  "each with in")


