# Elle API/Doc Requests — from bench friction analysis

## Context

In the relay benchmark (TCP pub/sub broker, 109 tests), the Claude Code
agent spent ~30 minutes hunting for how to write to a TCP connection.
The root cause: `tcp/accept` returns a port, but writing to it requires
`stream/write`, not `port/write`. The agent tried `port/write` first
(reasonable), failed, then searched through five different primitive
namespaces before finding `stream/write` in a separate doc section.

A second friction source: no synchronous stderr primitive. The agent
built a 10-line FFI shim (`ffi/malloc` + byte loop + `c-write(2, ...)`)
just to print "error: no port specified" to stderr.

## Request 1: TCP connection aliases

Add `tcp/write`, `tcp/read`, `tcp/read-line`, `tcp/flush` as aliases
for their `stream/` counterparts when used on a TCP connection (the
port returned by `tcp/accept` or `tcp/connect`).

This puts the entire TCP workflow in one namespace:

```lisp
(def listener (tcp/listen "0.0.0.0" 8080))
(def conn (tcp/accept listener))
(def line (tcp/read-line conn))    # instead of stream/read-line
(tcp/write conn "ECHO: ")         # instead of stream/write
(tcp/write conn line)
(tcp/write conn "\n")
(tcp/flush conn)                  # instead of stream/flush
(tcp/read conn 4096)              # instead of stream/read
(port/close conn)
```

The `stream/` names should keep working — these are aliases, not
replacements. The `tcp/` names just make discovery natural: if you
got the connection from `tcp/accept`, you write to it with `tcp/write`.

## Request 2: Synchronous stderr — `eprint` and `eprintln`

Add `eprint` and `eprintln` that write to fd 2 synchronously, like
`display` and `print` write to stdout. No yield, no event loop required.

```lisp
(eprintln "error: no port specified")   # writes to stderr + newline
(eprint "warning: ")                    # writes to stderr, no newline
```

Every single Elle benchmark implementation that needs stderr output
currently does this:

```lisp
(def libc (ffi/native nil))
(ffi/defbind c-write libc "write" :ssize @[:int :ptr :size])
(defn write-stderr [msg]
  (def buf (ffi/malloc (+ (length (bytes msg)) 1)))
  (var i 0)
  (def b (bytes msg))
  (while (< i (length b))
    (ffi/write (ptr/add buf i) :u8 (get b i))
    (assign i (+ i 1)))
  (ffi/write (ptr/add buf i) :u8 10)
  (c-write 2 buf (+ (length b) 1))
  (ffi/free buf))
```

That's 10 lines of FFI boilerplate for `fputs(msg, stderr)`.

## Request 3: QUICKSTART.md restructure (I can do this one)

Once the aliases land, I'll update QUICKSTART.md to:
- Document `tcp/write`, `tcp/read`, `tcp/read-line`, `tcp/flush` next
  to `tcp/listen`, `tcp/accept`, `tcp/connect`
- Replace the incomplete TCP example with a full echo loop
- Separate the functional stream combinators (`stream/map` etc.) from
  the I/O stream operations
- Document `eprint`/`eprintln` next to `print`/`display`
