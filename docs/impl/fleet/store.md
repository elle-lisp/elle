# Fleet: references and the store

<!-- audited: 2026-10-22 -->

What crosses a machine boundary in a fleet, where its bytes live, and the key
that names a task.

[The fleet design](../fleet.md) owns the argument this serves. Nothing here is
implemented.

## A reference names content by hash

Everything that crosses a machine boundary is a **reference**: an immutable
struct that names content by hash.

```text
{:hash  "…"          # BLAKE3 of the image bytes (plugin/hash)
 :size  1024
 :tier  :inline      # :inline | :store
 :bytes <bytes>}     # present only for :inline
```

Every reference names an image. Code is an environment image, and an argument
or a result is a data image, which `image/encode` writes and `image/decode`
hydrates ([sending a region](../send.md)). Both are locked to the build that
wrote them, so a reference crosses only between processes of one fingerprint
([workers](workers.md)).

A payload under 1 KiB rides inline in the reference itself. Everything else
lives in the **store**, keyed by hash. The store is `fleet:blob:<hash>` in
Redis first. A filesystem tier or an S3 tier (through `std/aws`) sits behind the
same two functions, `store-get` and `store-put`, for when blobs outgrow Redis.

Content addressing deduplicates for free. The same argument uploads once, and
ten thousand tasks that share one environment image store it once.

## A blob is a cache entry

A blob is a cache entry, not a record. Any blob can expire under a TTL policy,
and a client that still holds the value uploads it again. Result retention is a
per-fleet configuration.

Expiry has named consequences, and neither is ever a silent `nil`:

- A fetch whose blob has expired fails as `:vacated`, naming the hash. A holder
  of the value uploads it again and retries, and a vacated `fleet:pure` result
  recomputes.
- A fetch whose bytes do not hash to their key fails as `:corrupt`, before any
  decode.

## Task identity

```text
task-key = BLAKE3(env-hash ‖ entry ‖ arg-hash₁ ‖ … ‖ arg-hashₙ)
```

Dumps are deterministic, so `env-hash` means something: one session state
produces one set of image bytes. The same code and the same arguments therefore
make the same task key, across machines and across days. The key is the unit of
three things:

- **memoization**: `fleet:pure` reads a result by key
  ([orchestration](orchestration.md));
- **in-flight deduplication**: a duplicate submission of a running task
  subscribes to the existing result;
- **durability**: a result is queryable by key long after its client exits.

dill could not give code a stable hash, and an image dump can, so the key keeps
the content-addressing promise slay made.
