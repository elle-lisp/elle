(elle/epoch 11)
## tests/elle/port-shortread-framing.lisp
##
## Regression: read-line followed by read-exact on a binary stream must
## reassemble a payload that spans multiple TCP segments byte-for-byte,
## even when the read-line's recv over-read past the line terminator.
##
## This is the framing the redis client depends on (resp-read does
## `port/read-line` for the `$<len>\r\n` header, then
## `port/read-exact (+ len 2)` for the bulk body), reproduced over a
## plain loopback socket so it runs without a live Redis —
## redis-short-read.lisp covers the same property end-to-end when a
## server is available.
##
## Root cause (closed): when read-line's recv returned the header line
## PLUS the first chunk of the body, the leftover body bytes were stashed
## in the port's fd_state buffer.  The following binary read-exact set
## `read_buffered` to that leftover length — which both shrank the kernel
## read AND offset the kernel write to dst+read_buffered — yet left the
## leftover sitting in fd_state.  The completion's shift-prepend path then
## moved the kernel data as if it began at dst[0] (it began at
## dst+read_buffered), stranding `read_buffered` zero bytes in the middle
## of the reassembled buffer.  Symptom: byte-exact LENGTH but corrupted
## CONTENT, diverging at the offset equal to the read-line over-read
## (e.g. "unexpected RESP prefix" once the corruption desynchronised the
## next reply).
##
## Fix: at submission, move the buffered prefix into the fiber buffer at
## offset 0 and clear fd_state (mirroring the read-line no-newline path),
## so the completion sees an empty fd_state buffer and does no shift.

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
