(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## LSP Compliance Tests
##
## The original Rust compliance tests (integration/compliance.rs) validated
## JSON-RPC message structures using serde_json. Those tests verify protocol
## format compliance and don't exercise the Elle runtime — they test Rust
## data structures directly and remain in Rust.
##
## This file exists for completeness of the migration.

(assert-true true "compliance placeholder")
