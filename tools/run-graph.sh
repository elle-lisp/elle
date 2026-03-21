#!/bin/bash
# run-graph.sh — run elle-graph.lisp
# oxigraph's RocksDB dependency requires extra static TLS for dlopen
DIR="$(cd "$(dirname "$0")" && pwd)"
export GLIBC_TUNABLES=glibc.rtld.optional_static_tls=16384
elle "$DIR/elle-graph.lisp" "$@"
