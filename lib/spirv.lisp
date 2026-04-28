(elle/epoch 9)
## lib/spirv.lisp — SPIR-V bytecode emitter
##
## Generates SPIR-V compute shaders at runtime. No GLSL, no offline
## compilation — Elle code builds the bytecode directly.
##
## Usage:
##   (def spv ((import "std/spirv")))
##   (def bytecode (spv:compute 256 3 (fn [s]
##     (let* [[id (s:global-id)]
##            [a  (s:load 0 id)]
##            [b  (s:load 1 id)]]
##       (s:store 2 id (s:fadd a b))))
##     f32-bits-fn))

(fn []

  ## ── SPIR-V constants ────────────────────────────────────────────

  (def magic 0x07230203)
  (def version 0x00010500)

  ## Opcodes
  (def op-capability 17)
  (def op-memory-model 14)
  (def op-entry-point 15)
  (def op-execution-mode 16)
  (def op-decorate 71)
  (def op-member-decorate 72)
  (def op-type-void 19)
  (def op-type-bool 20)
  (def op-type-int 21)
  (def op-type-float 22)
  (def op-type-vector 23)
  (def op-type-runtime-array 29)
  (def op-type-struct 30)
  (def op-type-pointer 32)
  (def op-type-function 33)
  (def op-constant 43)
  (def op-variable 59)
  (def op-function 54)
  (def op-function-end 56)
  (def op-label 248)
  (def op-return 253)
  (def op-access-chain 65)
  (def op-load 61)
  (def op-store 62)
  (def op-fadd 129)
  (def op-fsub 131)
  (def op-fmul 133)
  (def op-fdiv 136)
  (def op-fmod 141)
  (def op-composite-extract 81)
  (def op-convert-u-to-f 112)
  (def op-convert-f-to-u 109)
  (def op-iadd 128)
  (def op-isub 130)
  (def op-imul 132)
  (def op-sdiv 135)
  (def op-umod 137)
  (def op-select 169)
  (def op-ford-less-than 184)
  (def op-ford-greater-than 186)
  (def op-ford-less-equal 188)
  (def op-ford-equal 180)
  (def op-sless-than 177)
  (def op-sgreater-than 173)
  (def op-logical-and 167)
  (def op-logical-not 168)
  (def op-bitwise-or 197)
  (def op-bitwise-and 199)
  (def op-shift-left 196)
  (def op-shift-right 194)
  (def op-umin 205)
  (def op-bitcast 124)
  (def op-branch 249)
  (def op-branch-conditional 250)
  (def op-loop-merge 246)
  (def op-selection-merge 247)

  ## Storage classes
  (def sc-input 1)
  (def sc-function 7)
  (def sc-storage-buffer 12)

  ## Decorations
  (def dec-block 2)
  (def dec-binding 33)
  (def dec-descriptor-set 34)
  (def dec-offset 35)
  (def dec-builtin 11)
  (def dec-array-stride 6)

  ## BuiltIns
  (def builtin-global-invocation-id 28)

  ## Execution
  (def exec-gl-compute 5)
  (def exec-mode-local-size 17)
  (def fn-control-none 0)

  ## ── Encoding helpers ────────────────────────────────────────────

  (defn encode-word [buf w]
    "Append a u32 little-endian to @bytes."
    (push buf (bit/and w 0xff))
    (push buf (bit/and (bit/shift-right w 8) 0xff))
    (push buf (bit/and (bit/shift-right w 16) 0xff))
    (push buf (bit/and (bit/shift-right w 24) 0xff)))
  (defn string-word-count [s]
    (let [n (+ (length s) 1)]
      (int (ceil (/ (float n) 4.0)))))

  ## ── Instruction emission ────────────────────────────────────────

  (defn emit-inst [section opcode & words]
    "Emit a SPIR-V instruction into a section (@[] of u32 words)."
    (push section (bit/or (bit/shift-left (+ 1 (length words)) 16) opcode))
    (each w words
      (push section w)))
  (defn string-to-words [s]
    "Convert string to null-terminated 4-byte-padded SPIR-V word array."
    (let* [raw (@bytes s)
           _ (push raw 0)  ## null terminate
           rem (% (length raw) 4)]
      (when (not (= rem 0)) (repeat (- 4 rem) (push raw 0)))
      (map (fn [i]
             (bit/or (raw i) (bit/shift-left (raw (+ i 1)) 8)
               (bit/shift-left (raw (+ i 2)) 16)
               (bit/shift-left (raw (+ i 3)) 24))) (range 0 (length raw) 4))))
  (defn emit-entry-point [section exec-model fn-id name interface-ids]
    "Emit OpEntryPoint with embedded string."
    (let* [str-words (string-to-words name)
           wc (+ 3 (length str-words) (length interface-ids))]
      (push section (bit/or (bit/shift-left wc 16) op-entry-point))
      (push section exec-model)
      (push section fn-id)
      (each w str-words
        (push section w))
      (each id interface-ids
        (push section id))))

  ## ── Module builder ──────────────────────────────────────────────

  (defn make-module []
    @{:caps @[]
      :mem-model @[]
      :entry @[]
      :exec-mode @[]
      :decorations @[]
      :types @[]
      :functions @[]})

  ## ── Serialization ───────────────────────────────────────────────

  (defn serialize [m bound]
    (let [buf (@bytes)]
      (encode-word buf magic)
      (encode-word buf version)
      (encode-word buf 0)
      (encode-word buf bound)
      (encode-word buf 0)
      (each w (m :caps)
        (encode-word buf w))
      (each w (m :mem-model)
        (encode-word buf w))
      (each w (m :entry)
        (encode-word buf w))
      (each w (m :exec-mode)
        (encode-word buf w))
      (each w (m :decorations)
        (encode-word buf w))
      (each w (m :types)
        (encode-word buf w))
      (each w (m :functions)
        (encode-word buf w))
      buf))

  ## ── High-level: compute shader builder ──────────────────────────

  (defn compute [local-size-x num-buffers body-fn f32-bits]
    "Build a SPIR-V compute shader with f32 storage buffers.
   Returns immutable SPIR-V bytes."
    (def @next-id 1)
    (let* [m (make-module)
           id (fn []
                (let [n next-id]
                  (assign next-id (+ n 1))
                  n))  ## ── Standard types ──────────────────────────────
           void-t (id)
           f32-t (id)
           u32-t (id)
           uvec3-t (id)
           rta-t (id)
           fn-void-t (id)
           ptr-uvec3-in-t (id)
           ptr-f32-sb-t (id)
           ptr-f32-fn-t (id)
           ptr-u32-fn-t (id)  ## ── Per-buffer types ────────────────────────────
           buf-struct-ids (map (fn [_] (id)) (range num-buffers))
           buf-ptr-ids (map (fn [_] (id)) (range num-buffers))
           buf-var-ids (map (fn [_] (id)) (range num-buffers))  ## ── Constants and special vars ──────────────────
           const-zero (id)
           gid-var (id)  ## ── Function ────────────────────────────────────
           main-fn (id)
           entry-lbl (id)]
      (emit-inst (m :caps) op-capability 1)

      ## ── Memory model ────────────────────────────────
      (emit-inst (m :mem-model) op-memory-model 0 1)

      ## ── Entry point ─────────────────────────────────
      (emit-entry-point (m :entry) exec-gl-compute main-fn "main"
        (concat [gid-var] buf-var-ids))

      ## ── Execution mode ──────────────────────────────
      (emit-inst (m :exec-mode) op-execution-mode main-fn exec-mode-local-size
        local-size-x 1 1)

      ## ── Decorations ─────────────────────────────────
      (emit-inst (m :decorations) op-decorate gid-var dec-builtin
        builtin-global-invocation-id)
      (emit-inst (m :decorations) op-decorate rta-t dec-array-stride 4)
      (each i (range num-buffers)
        (let* [struct-id (buf-struct-ids i)
               var-id (buf-var-ids i)]
          (emit-inst (m :decorations) op-decorate struct-id dec-block)
          (emit-inst (m :decorations) op-member-decorate struct-id 0 dec-offset
            0)
          (emit-inst (m :decorations) op-decorate var-id dec-descriptor-set 0)
          (emit-inst (m :decorations) op-decorate var-id dec-binding i)))

      ## ── Types ───────────────────────────────────────
      (emit-inst (m :types) op-type-void void-t)
      (emit-inst (m :types) op-type-float f32-t 32)
      (emit-inst (m :types) op-type-int u32-t 32 0)
      (emit-inst (m :types) op-type-vector uvec3-t u32-t 3)
      (emit-inst (m :types) op-type-runtime-array rta-t f32-t)
      (emit-inst (m :types) op-type-function fn-void-t void-t)
      (emit-inst (m :types) op-type-pointer ptr-uvec3-in-t sc-input uvec3-t)
      (emit-inst (m :types) op-type-pointer ptr-f32-sb-t sc-storage-buffer f32-t)
      (emit-inst (m :types) op-type-pointer ptr-f32-fn-t sc-function f32-t)
      (emit-inst (m :types) op-type-pointer ptr-u32-fn-t sc-function u32-t)
      (each i (range num-buffers)
        (let [struct-id (buf-struct-ids i)]
          (emit-inst (m :types) op-type-struct struct-id rta-t)
          (emit-inst (m :types) op-type-pointer (buf-ptr-ids i)
            sc-storage-buffer struct-id)))

      ## ── Constants ───────────────────────────────────
      (emit-inst (m :types) op-constant u32-t const-zero 0)

      ## ── Global variables ────────────────────────────
      (emit-inst (m :types) op-variable ptr-uvec3-in-t gid-var sc-input)
      (each i (range num-buffers)
        (emit-inst (m :types) op-variable (buf-ptr-ids i) (buf-var-ids i)
          sc-storage-buffer))

      ## ── Function body ───────────────────────────────
      ## fn-vars: OpVariable with Function storage class (must be first in entry block)
      ## body: all other function instructions (emitted by body-fn)
      (let* [fn-vars @[]
             body @[]
             binop (fn [opcode result-type a b]
                     (let [r (id)]
                       (emit-inst body opcode result-type r a b)
                       r))
             bool-t-cell @[nil]
             ensure-bool (fn []
                           (when (nil? (bool-t-cell 0))
                             (let [bt (id)]
                               (emit-inst (m :types) op-type-bool bt)
                               (put bool-t-cell 0 bt)))
                           (bool-t-cell 0))
             cmp-f (fn [opcode a b]
                     (let* [bt (ensure-bool)
                            r (id)]
                       (emit-inst body opcode bt r a b)
                       r))
             s {:global-id (fn []
                             (let* [gv (id)
                                    ix (id)]
                               (emit-inst body op-load uvec3-t gv gid-var)
                               (emit-inst body op-composite-extract u32-t ix gv
                                 0)
                               ix))
                :load (fn [buf-idx elem-idx]
                        (let* [ptr (id)
                               val (id)]
                          (emit-inst body op-access-chain ptr-f32-sb-t ptr
                            (buf-var-ids buf-idx) const-zero elem-idx)
                          (emit-inst body op-load f32-t val ptr)
                          val))
                :store (fn [buf-idx elem-idx val]
                         (let [ptr (id)]
                           (emit-inst body op-access-chain ptr-f32-sb-t ptr
                             (buf-var-ids buf-idx) const-zero elem-idx)
                           (emit-inst body op-store ptr val)))
                :fadd (fn [a b] (binop op-fadd f32-t a b))
                :fsub (fn [a b] (binop op-fsub f32-t a b))
                :fmul (fn [a b] (binop op-fmul f32-t a b))
                :fdiv (fn [a b] (binop op-fdiv f32-t a b))
                :fmod (fn [a b] (binop op-fmod f32-t a b))
                :iadd (fn [a b] (binop op-iadd u32-t a b))
                :isub (fn [a b] (binop op-isub u32-t a b))
                :imul (fn [a b] (binop op-imul u32-t a b))
                :idiv (fn [a b] (binop op-sdiv u32-t a b))
                :umod (fn [a b] (binop op-umod u32-t a b))
                :flt (fn [a b] (cmp-f op-ford-less-than a b))
                :fgt (fn [a b] (cmp-f op-ford-greater-than a b))
                :fle (fn [a b] (cmp-f op-ford-less-equal a b))
                :feq (fn [a b] (cmp-f op-ford-equal a b))
                :slt (fn [a b] (cmp-f op-sless-than a b))
                :sgt (fn [a b] (cmp-f op-sgreater-than a b))
                :select (fn [cond tv fv]
                          (let [r (id)]
                            (emit-inst body op-select f32-t r cond tv fv)
                            r))
                :select-u (fn [cond tv fv]
                            (let [r (id)]
                              (emit-inst body op-select u32-t r cond tv fv)
                              r))
                :const-f (fn [val]
                           (let* [r (id)
                                  bits (f32-bits val)]
                             (emit-inst (m :types) op-constant f32-t r bits)
                             r))
                :const-u (fn [val]
                           (let [r (id)]
                             (emit-inst (m :types) op-constant u32-t r val)
                             r))
                :u2f (fn [val]
                       (let [r (id)]
                         (emit-inst body op-convert-u-to-f f32-t r val)
                         r))
                :f2u (fn [val]
                       (let [r (id)]
                         (emit-inst body op-convert-f-to-u u32-t r val)
                         r))  ## ── Local variables ────────────────────────
                :var-f (fn []
                         (let [v (id)]
                           (emit-inst fn-vars op-variable ptr-f32-fn-t v
                             sc-function)
                           {:id v :type f32-t}))
                :var-u (fn []
                         (let [v (id)]
                           (emit-inst fn-vars op-variable ptr-u32-fn-t v
                             sc-function)
                           {:id v :type u32-t}))
                :load-var (fn [v]
                            (let [r (id)]
                              (emit-inst body op-load (v :type) r (v :id))
                              r))
                :store-var (fn [v val] (emit-inst body op-store (v :id) val))  ## ── Control flow ───────────────────────────
                :block (fn [] (id))
                :begin-block (fn [lbl] (emit-inst body op-label lbl))
                :branch (fn [lbl] (emit-inst body op-branch lbl))
                :branch-cond (fn [cond then else]
                               (emit-inst body op-branch-conditional cond then
                                 else))
                :loop-merge (fn [merge cont]
                              (emit-inst body op-loop-merge merge cont 0))
                :selection-merge (fn [merge]
                                   (emit-inst body op-selection-merge merge 0))  ## ── Logical ops ────────────────────────────
                :logical-and (fn [a b]
                               (let* [bt (ensure-bool)
                                      r (id)]
                                 (emit-inst body op-logical-and bt r a b)
                                 r))
                :logical-not (fn [a]
                               (let* [bt (ensure-bool)
                                      r (id)]
                                 (emit-inst body op-logical-not bt r a)
                                 r))  ## ── Integer bitwise ops ────────────────────
                :ior (fn [a b] (binop op-bitwise-or u32-t a b))
                :iand (fn [a b] (binop op-bitwise-and u32-t a b))
                :ishl (fn [a b] (binop op-shift-left u32-t a b))
                :ishr (fn [a b] (binop op-shift-right u32-t a b))
                :umin (fn [a b]
                        (let* [cmp (cmp-f op-sless-than a b)]
                          (let [r (id)]
                            (emit-inst body op-select u32-t r cmp a b)
                            r)))
                :bitcast-u2f (fn [val]
                               (let [r (id)]
                                 (emit-inst body op-bitcast f32-t r val)
                                 r))
                :bitcast-f2u (fn [val]
                               (let [r (id)]
                                 (emit-inst body op-bitcast u32-t r val)
                                 r))}]
        (body-fn s)

        ## ── Assemble function ─────────────────────────────
        ## OpFunction + OpLabel entry + fn-vars + body + OpReturn + OpFunctionEnd
        (emit-inst (m :functions) op-function void-t main-fn fn-control-none
          fn-void-t)
        (emit-inst (m :functions) op-label entry-lbl)
        (each w fn-vars
          (push (m :functions) w))
        (each w body
          (push (m :functions) w))
        (emit-inst (m :functions) op-return)
        (emit-inst (m :functions) op-function-end))

      ## ── Serialize ───────────────────────────────────
      (bytes (serialize m next-id))))

  ## ── Export ───────────────────────────────────────────────────────
  {:compute compute})
