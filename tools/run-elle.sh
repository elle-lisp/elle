#!/bin/sh
# Wrapper that sets GLIBC_TUNABLES for plugins embedding C++ (oxigraph, syn)
export GLIBC_TUNABLES=glibc.rtld.optional_static_tls=16384
exec target/release/elle "$@"
