#!/usr/bin/env elle
(elle/epoch 9)
# ── microgpt: minimal GPT in Elle ───────────────────────────────
#
# Port of https://github.com/karpathy/microgpt
# Scalar autograd + character-level GPT trained on names.
#
# Usage: cargo run --release -- demos/microgpt/microgpt.lisp

(def rng (import "plugin/random"))
(def ag ((import "demos/microgpt/autograd.lisp")))
(def helpers ((import "demos/microgpt/helpers.lisp")))
(def gpt ((import "demos/microgpt/model.lisp") ag helpers rng))

# ── Data loading and tokenizer ──────────────────────────────────

(defn load-data [path]
  "Load names from file, return mutable array of non-empty trimmed strings."
  (let* [result @[]]
    (each line in (read-lines path)
      (let [trimmed (string/trim line)]
        (when (> (length trimmed) 0) (push result trimmed))))
    result))

(defn build-tokenizer [names]
  "Build char-level tokenizer. Returns struct with :char->id, :id->char, :vocab-size.
   BOS token is at the last index (also serves as EOS)."
  (let* [chars @{}
         char->id @{}
         id->char @{}]
    (each name in names
      (each ch in name
        (put chars ch true)))
    (def @idx 0)
    (each ch in (sort (keys chars))
      (put char->id ch idx)
      (put id->char idx ch)
      (assign idx (inc idx)))
    (put id->char idx "<BOS>")
    @{:char->id char->id :id->char id->char :vocab-size (inc idx) :bos idx}))

(defn tokenize [name tokenizer]
  "Tokenize a name into array of integer IDs. Prepends and appends BOS."
  (let* [bos tokenizer:bos
         ids @[bos]]
    (each ch in name
      (push ids (tokenizer:char->id ch)))
    (push ids bos)
    ids))

# ── Adam optimizer ──────────────────────────────────────────────

(defn make-adam [params lr beta1 beta2 eps]
  "Create Adam optimizer state."
  @{:params params
    :lr lr
    :beta1 beta1
    :beta2 beta2
    :eps eps
    :m (array/new (length params) 0.0)
    :v (array/new (length params) 0.0)
    :step 0})

(defn adam-step [opt lr-current]
  "One Adam update step."
  (let [step (inc opt:step)]
    (put opt :step step)
    (def @i 0)
    (while (< i (length opt:params))
      (let* [p (opt:params i)
             g (ag:v-grad p)
             m-new (+ (* opt:beta1 (opt:m i)) (* (- 1.0 opt:beta1) g))
             v-new (+ (* opt:beta2 (opt:v i)) (* (- 1.0 opt:beta2) (* g g)))
             m-hat (/ m-new (- 1.0 (pow opt:beta1 (float step))))
             v-hat (/ v-new (- 1.0 (pow opt:beta2 (float step))))]
        (put opt:m i m-new)
        (put opt:v i v-new)
        (put p
             :data (- (ag:v-data p)
                      (* lr-current (/ m-hat (+ (sqrt v-hat) opt:eps))))))
      (assign i (inc i)))))

# ── Training ────────────────────────────────────────────────────

(defn train [model tokenizer names num-steps lr]
  "Train the model."
  (let* [params (gpt:collect-params model)
         opt (make-adam params lr 0.85 0.99 0.00000001)
         n-names (length names)]
    (println "Parameters: " (length params))
    (def @step 0)
    (while (< step num-steps)
      (let* [tokens (tokenize (names (mod step n-names)) tokenizer)
             tokens (if (> (length tokens) (inc gpt:*block-size*))
                      (slice tokens 0 (inc gpt:*block-size*))
                      tokens)
             loss (gpt:cross-entropy-loss-incremental model tokens)
             lr-current (* lr (- 1.0 (/ (float step) (float num-steps))))]
        (ag:backward loss)
        (adam-step opt lr-current)
        (each p in params
          (put p :grad 0.0))
        (when (= (mod step 100) 0)
          (println (string/format "step {:>4d} / {} | loss {:.4f}"
                                  step
                                  num-steps
                                  (ag:v-data loss)))))
      (assign step (inc step)))))

# ── Inference ───────────────────────────────────────────────────

(defn softmax-floats [scores]
  "Numerically stable softmax over an array of floats."
  (def @max-val (scores 0))
  (each s in scores
    (when (> s max-val) (assign max-val s)))
  (map (fn [s] (exp (- s max-val))) scores))

(defn sample-token [logits temperature]
  "Sample next token from logits using temperature-scaled softmax."
  (let* [scaled (map (fn [l] (/ (ag:v-data l) temperature)) logits)
         exps (softmax-floats scaled)]
    (rng:weighted (->array (range (length exps))) (->array exps))))

(defn generate [model tokenizer n-samples temperature max-len]
  "Generate n-samples names using incremental forward pass."
  (repeat n-samples
          (let* [chars @[]
                 [kv-keys kv-values] (helpers:make-kv-caches gpt:*n-layer*)]
            (def @token-id tokenizer:bos)
            (def @pos 0)
            (block :gen
              (while (< pos max-len)
                (let* [logits (gpt:gpt-forward-token token-id
                         pos
                         kv-keys
                         kv-values
                         model)
                       next-tok (sample-token logits temperature)]
                  (when (= next-tok tokenizer:bos) (break :gen))
                  (assign token-id next-tok)
                  (push chars (tokenizer:id->char next-tok))
                  (assign pos (inc pos)))))
            (println " " (string/join chars "")))))

# ── Gradient check ──────────────────────────────────────────────

(defn check-grads []
  "Quick gradient correctness check."
  (let* [a (ag:make-value 3.0)
         b (ag:make-value 4.0)
         c (ag:v+ (ag:v* a b) (ag:vpow a 2.0))]
    (ag:backward c)  # dc/da = b + 2a = 4 + 6 = 10, dc/db = a = 3
    (when (> (abs (- (ag:v-grad a) 10.0)) 0.000001)
      (error (string/format "grad check failed: da = {} (expected 10.0)"
                            (ag:v-grad a))))
    (when (> (abs (- (ag:v-grad b) 3.0)) 0.000001)
      (error (string/format "grad check failed: db = {} (expected 3.0)"
                            (ag:v-grad b))))
    (println "Gradient check passed.")))

# ── Main ────────────────────────────────────────────────────────

(defn main []
  (rng:seed 42)
  (check-grads)
  (println "Loading data...")
  (let [names (rng:shuffle (load-data "demos/microgpt/input.txt"))]
    (println "Loaded " (length names) " names")
    (let [tokenizer (build-tokenizer names)]
      (println "Vocab size: " tokenizer:vocab-size)
      (println "Initializing model...")
      (let [model (gpt:init-model tokenizer:vocab-size)]
        (println "Training...")
        (let [start (clock/monotonic)]
          (train model tokenizer names 1000 0.01)
          (println (string/format "Training took {:.1f}s"
                                  (- (clock/monotonic) start))))
        (println "\nGenerated names:")
        (generate model tokenizer 20 0.5 16)))))

(main)
