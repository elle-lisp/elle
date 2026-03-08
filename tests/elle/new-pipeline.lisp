# Integration tests for the new Syntax → HIR → LIR compilation pipeline
#
# Migrated from tests/integration/new_pipeline.rs
# Tests that use eval_source() with value comparisons or .is_err()
# Tests that use compiles()/compile() stay in Rust

(import-file "tests/elle/assert.lisp")

# ============ Loop Tests ============

# test_each_simple
(assert-eq (let ((sum 0)) (each x '(1 2 3) (assign sum (+ sum x))) sum) 6
  "each simple")

# test_each_with_in
(assert-eq (let ((sum 0)) (each x in '(1 2 3) (assign sum (+ sum x))) sum) 6
  "each with in")


