(elle/epoch 12)
# audited: 2026-09-23
## lib/hash.lisp — hash a port, a fiber stream or a file through a hash plugin's incremental API.
## docs/libraries.md
##
## The module takes the plugin as its argument and calls only its :new,
## :update and :finalize, so any struct with those three fields can stand in.
##
## Usage:
##   (def hash-plugin (import "plugin/hash"))
##   (def h ((import "std/hash") hash-plugin))
##
##   (bytes->hex (h:file :sha256 "bigfile.bin"))
##   (bytes->hex (h:digest :blake3 port))
##   (bytes->hex (h:stream :md5 my-fiber-source))

(fn [plugin]

  ## ── Core ────────────────────────────────────────────────────────────

  (defn hash/stream [algorithm source]
    "Fold a fiber source through a hasher and return the digest.
     algorithm: keyword (:sha256, :blake3, :md5, etc.).
     source: fiber that yields string, bytes, or @bytes chunks."
    (plugin:finalize (stream/fold plugin:update (plugin:new algorithm) source)))

  ## ── Convenience ─────────────────────────────────────────────────────

  (defn hash/digest [algorithm port &named @chunk-size]
    "Hash what remains to be read from port, and return the digest bytes.
     Reads chunk-size bytes at a time, and leaves the port open.
     A read error raises here. Open the port with port/open-bytes: a text
     port decodes each read as UTF-8, and raises on any other bytes."
    (default chunk-size 8192)
    (def @ctx (plugin:new algorithm))
    (def @chunk (port/read port chunk-size))
    (while (not (nil? chunk))
      (assign ctx (plugin:update ctx chunk))
      (assign chunk (port/read port chunk-size)))
    (plugin:finalize ctx))

  (defn hash/file [algorithm path &named @chunk-size]
    "Hash the bytes of the file at path, whatever they hold.
     Opens the file as a binary port, hashes it, and closes it.
     Returns the digest bytes (or integer for crc32/xxh32/xxh64)."
    (default chunk-size 8192)
    (let [p (port/open-bytes path :read)]
      (defer
        (port/close p)
        (hash/digest algorithm p :chunk-size chunk-size))))

  ## ── Export struct ───────────────────────────────────────────────────
  {:stream hash/stream :digest hash/digest :file hash/file})
