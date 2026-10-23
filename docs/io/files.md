# Files

<!-- audited: 2026-10-21 -->

The `file/` functions that take a path: which move contents on the I/O pool, which touch only metadata, temporary directories, and sync.

[I/O](../io.md) holds ports and the scheduler that runs every call here.
`(doc name)` gives each function's arguments and answer.

## Moving contents

`file/read`, `file/lines`, `file/write`, `file/append`, `file/copy` and
`file/sync` move a file's contents. Each runs on the I/O thread pool on every
platform, so other fibers run while it waits for the disk. Each needs both the
`:fs` and the `:io` capability, as `port/open` does. A failure raises
`:io-error`, and its `:path` names the file.

```lisp
(with-temp-dir dir
  (let [log (path/join dir "log")]
    (assert (= (file/write log "one\n") true))
    (file/append log "two\n")
    (file/copy log (path/join dir "copy"))
    (assert (= (file/read (path/join dir "copy")) "one\ntwo\n"))
    (assert (= (file/lines log) (list "one" "two")))
    (let [[ok? err] (protect (file/read (path/join dir "missing")))]
      (assert (= (get err :error) :io-error))
      (assert (= (get err :path) (path/join dir "missing"))))))
```

A worker runs each of these but `file/sync` through blocking system calls,
which nothing can interrupt. `ev/timeout` and a cancel discard the answer, and
the worker runs on to the end of the call. On a fifo, a read waits for a
writer, and a write waits for a reader.

## Metadata

`file/stat`, `file/lstat`, `file/size`, `file/ls`, `file/mkdir`,
`file/mkdir-all`, `file/delete`, `file/delete-dir`, `file/delete-dir-all`,
`file/rename` and `file/mktempdir` only read or change metadata. They run on
the scheduler's thread, return at once, and need `:fs` alone.

## Syncing to storage

`(file/sync path)` flushes the file or directory at `path` to stable storage,
as `fsync(2)` does, and answers nil. It opens `path` for reading, so a path
that does not exist, or that the process cannot read, raises `:io-error`. A
path that names neither a file nor a directory, such as a fifo, raises
`:io-error` at once.

A rename is durable only after its directory is synced too:

```lisp
(with-temp-dir dir
  (let [tmp (path/join dir "state.tmp")
        final (path/join dir "state")]
    (file/write tmp "v2")
    (assert (nil? (file/sync tmp)) "sync the new contents first")
    (file/rename tmp final)
    (assert (nil? (file/sync dir)) "then the directory that holds the rename")
    (assert (= (file/read final) "v2"))
    (let [[ok? err] (protect (file/sync (path/join dir "missing")))]
      (assert (not ok?))
      (assert (= (get err :error) :io-error)))))
```

## Temporary files

`file/mktempdir` creates a directory with a unique name under the platform
temp root, and returns its path. The root is `TMPDIR` on Unix, the per-user
folder on macOS, and `%TEMP%` on Windows, so a script never hardcodes `/tmp`.
Point `TMPDIR` at a tmpfs such as `/dev/shm` to keep scratch I/O in RAM. The
runtime makes each name from the process id and a counter, and retries on a
collision, so two processes cannot race to one name as fixed scratch
filenames do.

`with-temp-dir` scopes one to a body and deletes it on the way out —
recursively, error or not. `file/delete-dir-all` is the underlying
recursive delete (`file/delete-dir` only removes empty directories).

```lisp
(with-temp-dir dir
  (file/write (path/join dir "scratch.txt") "data")
  (assert (= (file/read (path/join dir "scratch.txt")) "data")))
```
