#!/bin/bash
# run-graph.sh — run elle-graph.lisp
# Note: elle handles GLIBC_TUNABLES internally via re-exec; this wrapper
# is kept for backward compatibility but the env var is no longer needed.
DIR="$(cd "$(dirname "$0")" && pwd)"
elle "$DIR/elle-graph.lisp" "$@"
