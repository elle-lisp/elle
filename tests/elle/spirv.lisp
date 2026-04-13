# SPIR-V builder tests
#
# Tests the lib/spirv.lisp builder, including control flow extensions
# (loops, local variables, structured branches).

(def spv ((import "std/spirv")))

## ── Helper: IEEE 754 f32 bit pattern ──────────────────────
## Standalone f32-bits for testing (no vulkan plugin needed).
## Only needs to be a valid u32 — SPIR-V builder doesn't
## interpret the bits, just embeds them as OpConstant operands.
(defn f32-bits [f]
  (cond
    [(= f 0.0)  0]
    [(= f 0.5)  0x3F000000]
    [(= f 1.0)  0x3F800000]
    [(= f 4.0)  0x40800000]
    [true       0]))

## ── SPIR-V header constants ──────────────────────────────
(def spirv-magic 0x07230203)

## ── 1. Basic shader generation (existing features) ────────
(let* [[bytecode (spv:compute 256 2 (fn [s]
          (let* [[id (s:global-id)]
                 [a  (s:load 0 id)]
                 [b  (s:load 1 id)]]
            (s:store 1 id (s:fadd a b))))
          f32-bits)]
       [len (length bytecode)]]
  (assert (> len 20) "spirv: bytecode has content")
  ## Check magic number (first 4 bytes, little-endian)
  (let [[magic (bit/or (bytecode 0)
                 (bit/shift-left (bytecode 1) 8)
                 (bit/shift-left (bytecode 2) 16)
                 (bit/shift-left (bytecode 3) 24))]]
    (assert (= magic spirv-magic) "spirv: magic number correct")))

## ── 2. Integer arithmetic ─────────────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id (s:global-id)]
               [a  (s:const-u 42)]
               [b  (s:const-u 10)]
               [c  (s:iadd a b)]
               [d  (s:isub a b)]
               [e  (s:imul a b)]]
          (s:store 1 id (s:u2f c))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: int arithmetic compiles"))

## ── 3. Comparisons and select ─────────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id  (s:global-id)]
               [a   (s:load 0 id)]
               [b   (s:const-f 0.5)]
               [cmp (s:flt a b)]
               [r   (s:select cmp a b)]]
          (s:store 1 id r)))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: comparison + select compiles"))

## ── 4. Local variables (new) ──────────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id  (s:global-id)]
               [v   (s:var-f)]
               [val (s:load 0 id)]]
          (s:store-var v val)
          (s:store 1 id (s:load-var v))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: local variable load/store compiles"))

## ── 5. Integer local variable ─────────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id  (s:global-id)]
               [v   (s:var-u)]
               [c   (s:const-u 7)]]
          (s:store-var v c)
          (s:store 1 id (s:u2f (s:load-var v)))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: u32 local variable compiles"))

## ── 6. Control flow: simple branch ────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id   (s:global-id)]
               [skip (s:block)]
               [done (s:block)]
               [a    (s:load 0 id)]
               [cmp  (s:flt a (s:const-f 0.5))]]
          (s:selection-merge done)
          (s:branch-cond cmp skip done)
          (s:begin-block skip)
          (s:store 1 id (s:const-f 1.0))
          (s:branch done)
          (s:begin-block done)
          (s:store 1 id (s:const-f 0.0))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: selection merge compiles"))

## ── 7. Control flow: loop ─────────────────────────────────
(let [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id       (s:global-id)]
               [counter  (s:var-u)]
               [zero     (s:const-u 0)]
               [one      (s:const-u 1)]
               [limit    (s:const-u 10)]
               [hdr      (s:block)]
               [body     (s:block)]
               [cont     (s:block)]
               [done     (s:block)]]
          (s:store-var counter zero)
          (s:branch hdr)
          ## header
          (s:begin-block hdr)
          (let* [[n   (s:load-var counter)]
                 [cmp (s:slt n limit)]]
            (s:loop-merge done cont)
            (s:branch-cond cmp body done))
          ## body
          (s:begin-block body)
          (s:store-var counter (s:iadd (s:load-var counter) one))
          (s:branch cont)
          ## continue
          (s:begin-block cont)
          (s:branch hdr)
          ## exit
          (s:begin-block done)
          (s:store 0 id (s:u2f (s:load-var counter)))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: loop compiles"))

## ── 8. Logical ops ────────────────────────────────────────
(let [[bytecode (spv:compute 256 2 (fn [s]
        (let* [[id (s:global-id)]
               [a  (s:load 0 id)]
               [b  (s:load 1 id)]
               [p  (s:flt a (s:const-f 1.0))]
               [q  (s:fgt b (s:const-f 0.0))]
               [r  (s:logical-and p q)]
               [nr (s:logical-not r)]]
          (s:store 1 id (s:select nr (s:const-f 1.0) (s:const-f 0.0)))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: logical ops compile"))

## ── 9. Signed greater-than ────────────────────────────────
(let [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id (s:global-id)]
               [a  (s:const-u 5)]
               [b  (s:const-u 3)]
               [c  (s:sgt a b)]]
          (s:store 0 id (s:select-u c (s:const-u 1) (s:const-u 0)))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: sgt compiles"))

## ── 10. Mandelbrot-style kernel ───────────────────────────
(let [[bytecode (spv:compute 256 3 (fn [s]
        (let* [[id       (s:global-id)]
               [cx       (s:load 0 id)]
               [cy       (s:load 1 id)]
               [zr       (s:var-f)]
               [zi       (s:var-f)]
               [iter     (s:var-u)]
               [max-iter (s:const-u 256)]
               [four     (s:const-f 4.0)]
               [zero-f   (s:const-f 0.0)]
               [zero-u   (s:const-u 0)]
               [one-u    (s:const-u 1)]
               [hdr      (s:block)]
               [body     (s:block)]
               [cont     (s:block)]
               [done     (s:block)]]
          ## init
          (s:store-var zr zero-f)
          (s:store-var zi zero-f)
          (s:store-var iter zero-u)
          (s:branch hdr)
          ## header
          (s:begin-block hdr)
          (let* [[r   (s:load-var zr)]
                 [i   (s:load-var zi)]
                 [r2  (s:fmul r r)]
                 [i2  (s:fmul i i)]
                 [mag (s:fadd r2 i2)]
                 [ok  (s:flt mag four)]
                 [n   (s:load-var iter)]
                 [lim (s:slt n max-iter)]
                 [go  (s:logical-and ok lim)]]
            (s:loop-merge done cont)
            (s:branch-cond go body done))
          ## body
          (s:begin-block body)
          (let* [[r  (s:load-var zr)]
                 [i  (s:load-var zi)]
                 [ri (s:fmul r i)]
                 [r2 (s:fmul r r)]
                 [i2 (s:fmul i i)]
                 [nr (s:fadd (s:fsub r2 i2) cx)]
                 [ni (s:fadd (s:fadd ri ri) cy)]]
            (s:store-var zr nr)
            (s:store-var zi ni)
            (s:store-var iter (s:iadd (s:load-var iter) one-u))
            (s:branch cont))
          ## continue
          (s:begin-block cont)
          (s:branch hdr)
          ## exit
          (s:begin-block done)
          (s:store 2 id (s:u2f (s:load-var iter)))))
        f32-bits)]]
  (assert (> (length bytecode) 100) "spirv: mandelbrot kernel compiles"))

## ── 11. Integer bitwise ops ────────────────────────────────
(let [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id  (s:global-id)]
               [a   (s:const-u 0xF0)]
               [b   (s:const-u 0x0F)]
               [c   (s:const-u 4)]
               [or-result  (s:ior a b)]
               [and-result (s:iand a b)]
               [shl-result (s:ishl b c)]
               [shr-result (s:ishr a c)]]
          ## store or-result: 0xF0 | 0x0F = 0xFF = 255
          (s:store 0 id (s:u2f or-result))))
        f32-bits)]]
  (assert (> (length bytecode) 20) "spirv: bitwise ops compile"))

## ── 12. Bitwise ops on GPU ────────────────────────────────
(let* [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id (s:global-id)]
               [a  (s:const-u 0xF0)]
               [b  (s:const-u 0x0F)]
               [r  (s:ior a b)]]
          (s:store 0 id (s:u2f r))))
        f32-bits)]
       [p (port/open "/tmp/elle-spirv-bitwise.spv" :write)]
       [_ (port/write p bytecode)]
       [_ (port/close p)]
       [result (subprocess/system "spirv-val" ["/tmp/elle-spirv-bitwise.spv"])]]
  (when (not (= (result :exit) 0))
    (eprintln "spirv-val stderr:" (result :stderr)))
  (assert (= (result :exit) 0) "spirv: bitwise ior validates"))

## ── 13. Bitcast u32→f32 on GPU ────────────────────────────
(let* [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id  (s:global-id)]
               [val (s:const-u 0x42280000)]  # f32 bit pattern for 42.0
               [as-f (s:bitcast-u2f val)]]
          (s:store 0 id as-f)))
        f32-bits)]
       [p (port/open "/tmp/elle-spirv-bitcast.spv" :write)]
       [_ (port/write p bytecode)]
       [_ (port/close p)]
       [result (subprocess/system "spirv-val" ["/tmp/elle-spirv-bitcast.spv"])]]
  (assert (= (result :exit) 0) "spirv: bitcast validates"))

## ── 14. umin on GPU ──────────────────────────────────────
(let* [[bytecode (spv:compute 256 1 (fn [s]
        (let* [[id (s:global-id)]
               [a  (s:const-u 999)]
               [b  (s:const-u 255)]
               [r  (s:umin a b)]]
          (s:store 0 id (s:u2f r))))
        f32-bits)]
       [p (port/open "/tmp/elle-spirv-umin.spv" :write)]
       [_ (port/write p bytecode)]
       [_ (port/close p)]
       [result (subprocess/system "spirv-val" ["/tmp/elle-spirv-umin.spv"])]]
  (assert (= (result :exit) 0) "spirv: umin validates"))

(println "All SPIR-V tests passed")
