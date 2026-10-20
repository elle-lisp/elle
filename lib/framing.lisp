(elle/epoch 15)
# audited: 2026-10-20
## A record cursor over any byte source: lines, delimited records and exact counts, each a view of one chunk.
## lib/framing.md
##
## Loaded via: (def framing ((import "std/framing")))
## Usage:      (framing:line (framing:open port))

(def default-size 65536)

## ── State ───────────────────────────────────────────────────────────────

(defn make-framer [read exact size buf]
  "A framer whose :read answers up to n bytes and nil at the end, whose
   :exact, when present, answers exactly n bytes or nil, and whose buffer
   starts as buf."
  @{:read read :exact exact :buf buf :pos 0 :size size :eof false})

(defn as-bytes [chunk]
  "chunk as immutable bytes: a string or mutable bytes is copied, bytes are not."
  (if (and (bytes? chunk) (not (mutable? chunk))) chunk (bytes chunk)))

(defn fiber-read [fib]
  "A read function over a fiber of chunks: each call resumes the fiber and
   answers its value, or nil once the fiber is done."
  (fn [n]
    (fiber/resume fib)
    (if (fiber/done? fib) nil (fiber/value fib))))

(defn framing-open [source &named @size]
  "A framer over source: a binary port, a function of one argument
   answering up to that many bytes and nil at the end, a fiber answering
   one chunk per resume, or bytes or a string in memory. :size is the chunk
   asked for, 65536 unless given. The framer never closes its source."
  (default size default-size)
  (cond
    (port? source)
      (begin
        (unless (= :binary (port/encoding source))
          (error {:error :framing-error
                  :reason :text-port
                  :encoding (port/encoding source)
                  :message "framing/open: the framer counts bytes; open the port with port/open-bytes or :encoding :binary"}))
        (make-framer (fn [n] (port/read source n))
                     (fn [n] (port/read-exact source n)) size (bytes)))
    (fiber? source) (make-framer (fiber-read source) nil size (bytes))
    (fn? source) (make-framer source nil size (bytes))
    (or (bytes? source) (string? source))
      (make-framer (fn [n] nil) nil size (as-bytes source))
    true (error {:error :framing-error
                 :reason :bad-source
                 :source source
                 :message "framing/open: the source is not a port, a function, a fiber, bytes or a string"})))

(defn fill [r]
  "Replace the buffer with the next chunk of the source; false at the end."
  (if r:eof
    false
    (let [chunk ((get r :read) r:size)]
      (if (nil? chunk)
        (begin
          (put r :eof true)
          (put r :buf (bytes))
          (put r :pos 0)
          false)
        (begin
          (put r :buf (as-bytes chunk))
          (put r :pos 0)
          true)))))

(defn join-parts [parts]
  "One bytes value from the parts of a record: the part itself when there
   is one, so a record inside a chunk is a view of it."
  (case (length parts)
    0 (bytes)
    1 (get parts 0)
    (apply concat (freeze parts))))

(defn last-bytes [tail piece n]
  "The last n bytes of tail followed by piece."
  (let [all (concat tail piece)]
    (slice all (max 0 (- (length all) n)))))

## ── Records ─────────────────────────────────────────────────────────────

(defn framing-until [r delim]
  "The bytes before the next delim, which is bytes, a string or one byte as
   an integer; the delimiter is consumed. At the end, the rest, or nil when
   nothing remains."
  (let [d (if (int? delim) (bytes delim) (as-bytes delim))
        dlen (length d)
        parts @[]]
    (def @tail (bytes))
    (def @result nil)
    (def @done false)
    (while (not done)
      (let [buf r:buf
            pos r:pos
            i (bytes/find buf d pos)]
        (if i
          (begin
            (push parts (slice buf pos i))
            (put r :pos (+ i dlen))
            (assign result (join-parts parts))
            (assign done true))
          (begin
            (push parts (slice buf pos))
            (put r :pos (length buf))
            (when (> dlen 1)
              (assign tail (last-bytes tail (slice buf pos) (dec dlen))))
            (if (not (fill r))
              (let [joined (join-parts parts)]
                (assign result (if (empty? joined) nil joined))
                (assign done true))
              (when (and (> dlen 1) (not (empty? tail)))
                (let [head (slice r:buf 0 (min (dec dlen) (length r:buf)))
                      win (concat tail head)
                      j (bytes/find win d 0)]
                  (when (and j (< j (length tail)))
                    (let [cut (- (length tail) j)
                          joined (join-parts parts)]
                      (put r :pos (- dlen cut))
                      (assign result (slice joined 0 (- (length joined) cut)))
                      (assign done true))))))))))
    result))

(defn framing-line [r]
  "The next line as bytes, without its newline and a carriage return before
   it; a partial last line whole; nil at the end."
  (let [rec (framing-until r 10)]
    (if (and rec (not r:eof))
      (let [n (length rec)]
        (if (and (> n 0) (= 13 (get rec (dec n)))) (slice rec 0 (dec n)) rec))
      rec)))

(defn framing-exact [r n]
  "Exactly n bytes; nil when the source ends first, and the bytes it held
   go with it."
  (let [buf r:buf
        pos r:pos
        have (- (length buf) pos)]
    (if (>= have n)
      (begin
        (put r :pos (+ pos n))
        (slice buf pos (+ pos n)))
      (let [parts @[]]
        (when (> have 0) (push parts (slice buf pos)))
        (put r :pos (length buf))
        (def @need (- n have))
        (if r:exact
          (let [more ((get r :exact) need)]
            (when more
              (push parts (as-bytes more))
              (join-parts parts)))
          (block (def @result nil)
            (def @done false)
            (while (not done)
              (if (not (fill r))
                (assign done true)
                (let [b r:buf
                      blen (length b)]
                  (if (>= blen need)
                    (begin
                      (push parts (slice b 0 need))
                      (put r :pos need)
                      (assign result (join-parts parts))
                      (assign done true))
                    (begin
                      (push parts b)
                      (put r :pos blen)
                      (assign need (- need blen)))))))
            result))))))

(defn framing-up-to [r n]
  "Up to n bytes of what the framer holds, after one fill when it holds
   nothing; nil at the end."
  (if (and (= r:pos (length r:buf)) (not (fill r)))
    nil
    (let [buf r:buf
          pos r:pos
          end (min (length buf) (+ pos n))]
      (put r :pos end)
      (slice buf pos end))))

(defn framing-rest [r]
  "Everything to the end as one value; empty bytes when nothing remains."
  (let [parts @[]]
    (when (< r:pos (length r:buf)) (push parts (slice r:buf r:pos)))
    (put r :pos (length r:buf))
    (while (fill r)
      (push parts r:buf)
      (put r :pos (length r:buf)))
    (join-parts parts)))

## ── Streams ─────────────────────────────────────────────────────────────

(defn framing-lines [r]
  "A fiber yielding each line of r, ending at the end of the source. It
   closes nothing."
  (fiber/new (fn []
               (forever
                 (let [line (framing-line r)]
                   (if (nil? line) (break) (yield line))))) |:yield|))

(defn framing-records [r delim]
  "A fiber yielding each record of r before delim, ending at the end of the
   source. It closes nothing."
  (fiber/new (fn []
               (forever
                 (let [rec (framing-until r delim)]
                   (if (nil? rec) (break) (yield rec))))) |:yield|))

## ── Exports ─────────────────────────────────────────────────────────────

(fn []
  {:open framing-open
   :line framing-line
   :until framing-until
   :exact framing-exact
   :up-to framing-up-to
   :rest framing-rest
   :lines framing-lines
   :records framing-records})
