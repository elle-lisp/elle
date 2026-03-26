## lib/hash.lisp — streaming hash convenience functions
##
## Provides high-level helpers for hashing ports, coroutine streams,
## and files using the elle-hash plugin's incremental API.
##
## Dependencies:
##   - elle-hash plugin loaded via (import-file "path/to/libelle_hash.so")
##   - port/chunks, stream/fold from stdlib
##
## Usage:
##   (def hash-plugin (import-file "target/release/libelle_hash.so"))
##   (def h ((import-file "lib/hash.lisp") hash-plugin))
##
##   (bytes->hex (h:file :sha256 "bigfile.bin"))
##   (bytes->hex (h:digest :blake3 port))
##   (bytes->hex (h:stream :md5 my-coroutine-source))

(fn [plugin]

  ## ── Core ────────────────────────────────────────────────────────────

  (defn hash/stream [algorithm source]
    "Fold a coroutine source through a hasher and return the digest.
     algorithm: keyword (:sha256, :blake3, :md5, etc.).
     source: coroutine that yields string, bytes, or @bytes chunks."
    (plugin:finalize
      (stream/fold plugin:update (plugin:new algorithm) source)))

  ## ── Convenience ─────────────────────────────────────────────────────

  (defn hash/digest [algorithm port &named chunk-size]
    "Hash an open port's remaining contents. Returns the digest bytes.
     Does not close the port."
    (default chunk-size 8192)
    (hash/stream algorithm (port/chunks port chunk-size)))

  (defn hash/file [algorithm path &named chunk-size]
    "Hash a file by path. Opens, hashes, and closes the file.
     Returns the digest bytes (or integer for crc32/xxh32/xxh64)."
    (default chunk-size 8192)
    (let [[p (port/open path :read)]]
      (defer (port/close p)
        (hash/digest algorithm p :chunk-size chunk-size))))

  ## ── Export struct ───────────────────────────────────────────────────
  {:stream hash/stream
   :digest hash/digest
   :file   hash/file})
