(elle/epoch 12)
# audited: 2026-09-29
# read-line then read-exact on a binary stream reassembles a payload spanning several TCP segments byte for byte.
# docs/io.md
#
## This is the framing the redis client depends on (resp-read does
## `port/read-line` for the `$<len>\r\n` header, then
## `port/read-exact (+ len 2)` for the bulk body), reproduced over a
## plain loopback socket so it runs without a live Redis —
## redis-short-read.lisp covers the same property end-to-end when a
## server is available.
##
## The rule: when read-line's recv returns the header line PLUS the first chunk
## of the body, the leftover body bytes wait in the port's fd_state buffer. A
## binary read-exact submitted next moves that prefix into the fiber buffer at
## offset 0 and clears fd_state (as the read-line no-newline path does), so the
## completion sees an empty fd_state buffer and does no shift.
##
## The counter-factual sets `read_buffered` to the leftover length and leaves
## the leftover in fd_state. That shrinks the kernel read and offsets its write
## to dst+read_buffered, and the completion's shift-prepend then moves the kernel
## data as if it began at dst[0], stranding `read_buffered` zero bytes in the
## middle. The LENGTH is exact and the CONTENT is corrupt, diverging at the
## offset equal to the read-line over-read — "unexpected RESP prefix" once the
## corruption desynchronizes the next reply.

## A loopback server that writes `$<len>\r\n<payload>\r\n` `n` times,
## then a client that frames each reply with read-line + read-exact.
(defn frame-roundtrip [value-size n-frames]
  (let [listener (tcp/listen "127.0.0.1" 0)
        port-num (parse-int (get (string/split (port/path listener) ":") 1))
        payload (let [@buf @""
                      @i 0]
                  (while (< i value-size)
                    (push buf (string (mod i 10)))
                    (assign i (+ i 1)))
                  (freeze buf))
        frame (concat "$" (string value-size) "\r\n" payload "\r\n")]
    (ev/spawn (fn []
                (let [conn (tcp/accept listener)]
                  (def @k 0)
                  (while (< k n-frames)
                    (port/write conn frame)
                    (assign k (+ k 1)))
                  (port/flush conn)
                  (ev/sleep 0.2)
                  (port/close conn))))
    (let [client (tcp/connect "127.0.0.1" port-num)]
      (def @r 0)
      (while (< r n-frames)
        (let [line (port/read-line client)]
          (assert (= (get line 0) "$")
                  (concat "round " (string r) ": expected $ header, got " line))
          (let [len (parse-int (slice line 1))
                data (port/read-exact client (+ len 2))]
            (assert (not (nil? data))
                    (concat "round " (string r) ": read-exact returned nil"))
            (assert (= (length data) (+ len 2))
                    (concat "round " (string r) ": got " (string (length data))
                            " bytes, want " (string (+ len 2))))
            (let [body (string (slice data 0 len))]
              (assert (= body payload)
                      (concat "round " (string r)
                              ": payload corrupted (length ok, content wrong)")))))
        (assign r (+ r 1)))
      (port/close client))))

## 200 KiB exceeds the loopback recv buffer (~64 KiB default), so the
## kernel almost always splits both the header+body recv and the body
## itself across multiple segments.  Multiple frames make a single
## round's leftover corrupt the next round if framing desyncs.
(frame-roundtrip 200000 3)
(println "  1. 200 KiB × 3 frames: byte-exact")

## A frame whose body is just a few bytes longer than a typical recv,
## exercising the small-leftover case.
(frame-roundtrip 5000 4)
(println "  2. 5 KiB × 4 frames: byte-exact")

(println "port-shortread-framing: all tests passed")
