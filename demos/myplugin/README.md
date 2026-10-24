# myplugin

<!-- audited: 2026-10-23 -->

The smallest plugin the stable ABI can express: one primitive,
`myplugin/hello`, which answers `"hello"`.

[The plugin recipe](../../docs/cookbook/plugins.md) walks through
[lib.rs](src/lib.rs) and then imports the crate as `plugin/myplugin`, so the
recipe is the document for this crate. `make myplugin` builds it, and
`make doctest` needs it built.
