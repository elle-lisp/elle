# Files

<!-- audited: 2026-11-16 -->

Every `file/` function and `path/` probe that touches the disk runs on the I/O pool; also temporary directories and sync.

[I/O](../io.md) holds ports and the scheduler that runs every call here.
`(doc name)` gives each function's arguments and answer.

## On the I/O pool

Every `file/` function that takes a path runs on the I/O thread pool, on every
platform, so other fibers run while it waits for the disk. So do the four
`path/` probes that touch the disk:

| Calls | Do |
|-------|----|
| `file/read`, `file/lines`, `file/write`, `file/append`, `file/copy`, `file/sync` | move a file's contents |
| `file/stat`, `file/lstat`, `file/size`, `file/ls` | read metadata |
| `file/mkdir`, `file/mkdir-all`, `file/delete`, `file/delete-dir`, `file/delete-dir-all`, `file/rename`, `file/mktempdir` | change metadata |
| `path/exists?`, `path/file?`, `path/dir?`, `path/canonicalize` | probe a path |

Each needs both the `:fs` and the `:io` capability, as `port/open` does
([capabilities](../signals/capabilities.md)). A failure raises `:io-error`,
and its `:path` names the file: the old path, for a copy or a rename. A probe
answers false for a path it cannot reach, and never raises. `path/cwd` and
`path/absolute` read only the process's working directory, and return at once.

```lisp
(with-temp-dir dir
  (let [log (path/join dir "log")]
    (assert (= (file/write log "one\n") true))
    (file/append log "two\n")
    (file/copy log (path/join dir "copy"))
    (assert (= (file/read (path/join dir "copy")) "one\ntwo\n"))
    (assert (= (file/lines log) (list "one" "two")))
    (assert (= (file/size log) 8))
    (assert (= (get (file/stat dir) :file-type) "dir"))
    (assert (= (sort (file/ls dir)) (list "copy" "log")))
    (let [[ok? err] (protect (file/read (path/join dir "missing")))]
      (assert (= (get err :error) :io-error))
      (assert (= (get err :path) (path/join dir "missing"))))
    (let [[ok? err] (protect (file/stat (path/join dir "missing")))]
      (assert (= (get err :path) (path/join dir "missing"))))
    (assert (not (path/exists? (path/join dir "missing"))))))
```

A metadata call reaches the scheduler as a read does. A fiber whose mask names
`:io` catches its request, and `:deny |:io|` stops it:

```lisp
(with-temp-dir dir
  (let [f (fiber/new (fn [] (file/stat dir)) |:io|)]
    (assert (io-request? (fiber/resume f)) "the stat waits on the scheduler"))
  (let [f (fiber/new (fn [] (path/exists? dir)) |:io :error| :deny |:io|)]
    (fiber/resume f)
    (assert (= (get (fiber/value f) :primitive) "path/exists?")
            "a fiber denied :io cannot probe the disk")))
```

A worker runs each of these but `file/sync` through blocking system calls,
which nothing can interrupt. `ev/timeout` and a cancel discard the answer, and
the worker runs on to the end of the call. On a fifo, a read waits for a
writer, and a write waits for a reader. A stat or a probe of a fifo opens
nothing and answers at once, and so does a list of a directory that holds one.

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
recursively, error or not. `file/delete-dir-all` is the underlying recursive
delete (`file/delete-dir` only removes empty directories).

```lisp
(with-temp-dir dir
  (file/write (path/join dir "scratch.txt") "data")
  (assert (= (file/read (path/join dir "scratch.txt")) "data")))
```

The delete runs when `ev/abort`, `ev/timeout` or `ev/race` ends the body too.
It waits on the disk as any other call does, so it finishes after the abort
returns. Join the fiber to wait for it:

```lisp
(let [cell @[nil]
      body (ev/spawn (fn [] (with-temp-dir dir (put cell 0 dir) (ev/sleep 10))))]
  (while (nil? (get cell 0)) (ev/sleep 0.001))
  (ev/abort body)
  (ev/join-protected body)
  (assert (not (path/exists? (get cell 0))) "the aborted body's directory is gone"))
```
