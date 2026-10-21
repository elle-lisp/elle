# The embedded blob

<!-- audited: 2026-10-21 -->

The boot image a release executable carries: the two-stage build that embeds
it, and how a start finds its bytes.

[boot.md](boot.md) owns the boot configuration and the policy that tries the
blob first.

## Two stages build it

A release `make elle` builds an `elle` and an `elle-rig` that each carry the
image of their own sources, so the first start hydrates it as every later start
does. A debug build, a plain `cargo build` and the variant builds carry none.

The Makefile builds twice. Stage 1 is an ordinary release build in
`target/stage1`, and its `elle image dump-boot` writes the blob to
`target/stage1/boot.image`. Stage 2 builds again with `ELLE_BOOT_BLOB` naming
that file. [build.rs](../../../build.rs) hands the path to the library, and
[blob.rs](../../../src/image/boot/blob.rs) compiles the blob in with
`include_bytes!`. Every release cargo call in the Makefile that links the
default-feature library passes the same variable.

Stage 1 has a target directory of its own because cargo keeps one build of a
library per profile and feature set. A change to the variable rebuilds that
library, so in one shared directory every `make elle` would build the library
twice. A variant build passes no variable. A blob is what one build dumped, and
the fingerprint does not record the cargo features ([format.md](format.md)).

Stage 1 replaces the blob file only when the dump's bytes differ. Cargo
rebuilds the library when the file is newer than the last build, through both
`include_bytes!` and the build script's rerun check. So rewriting the same
bytes would rebuild the library at every `make elle`.

`make elle` then checks stage 2, and fails when either check fails:

- Its own `elle image dump-boot` writes the embedded bytes again, byte for
  byte. Embedding changes the binary and none of the fingerprint's inputs, and
  the dump is deterministic ([image.md](../image.md)). So a difference means
  the two stages compiled the library differently.
- A `--cache=` start with `--trace=boot` that spawns a worker prints a
  `blob-map` mark and two `image-hydrate` marks. The start mapped the blob from
  its own executable, and the worker hydrated the same descriptor.

## A start finds the bytes

The blob sits behind a `#[repr(align(65536))]` wrapper, so its virtual address
is 64 KiB-aligned, and load-segment congruence makes its file offset aligned
too. The constant is the largest base page any supported host reports, because
the alignment a blob needs is the host's page size. No smaller constant covers
a 64 KiB arm64 kernel. A blob dumped on a 4 KiB host still fails the
fingerprint on a 16 KiB one ([format.md](format.md)), and the start falls back.

A start finds the blob in the file that holds it. The offset is the static's
address, less the base of the segment that holds it, plus that segment's file
offset. `dl_iterate_phdr` reports the segments on Linux and Android, and the
Mach-O load commands report them on macOS. Linux and Android open the
executable through `/proc/self/exe`, which names the running file even after a
rebuild replaced its path. A shared object, such as the embedding demo's
library, opens by the path the loader reports, and so does a macOS executable.

The bytes at that offset must begin with the blob's header block as it sits in
memory, and the offset must fall on a base page. A rebuild that replaced a
macOS path breaks the first rule, and so does a universal Mach-O, which holds
the slice at an offset its load commands do not show. When a rule fails, or
the file does not open, the start copies the blob into an anonymous file
([image.md](../image.md)) and hydrates that. It prints `blob-copy` in place of
`blob-map`. The copy costs a write of the whole blob, and its pages are this
process's alone.
