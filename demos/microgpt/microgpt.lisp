#!/usr/bin/env elle
(elle/epoch 6)
# ── microgpt: minimal GPT in Elle ───────────────────────────────
#
# Port of https://github.com/karpathy/microgpt
# Scalar autograd + character-level GPT trained on names.
#
# Usage: cargo run --release -- demos/microgpt/microgpt.lisp

(def rng (import "plugin/random"))
(def ag      ((import "demos/microgpt/autograd.lisp")))
(def helpers ((import "demos/microgpt/helpers.lisp")))
(def gpt     ((import "demos/microgpt/model.lisp") ag helpers rng))

(def {:make-value make-value :v-data v-data :v-grad v-grad
      :v+ v+ :v* v* :vpow vpow :backward backward} ag)
(def {:make-kv-caches make-kv-caches} helpers)
(def {:init-model init-model :collect-params collect-params
      :gpt-forward-token gpt-forward-token
      :cross-entropy-loss-incremental cross-entropy-loss-incremental
      :*block-size* *block-size* :*n-layer* *n-layer*} gpt)

# ── Data loading and tokenizer ──────────────────────────────────

(defn load-data [path]
  "Load names from file, return mutable array of non-empty trimmed strings."
  (let* ([result @[]])
    (each line in (read-lines path)
      (let* ([trimmed (string/trim line)])
        (when (> (length trimmed) 0)
          (push result trimmed))))
    result))

(defn build-tokenizer [names]
  "Build char-level tokenizer. Returns struct with :char->id, :id->char, :vocab-size.
   BOS token is at the last index (also serves as EOS)."
  (let* ([chars @{}])
    (each name in names
      (each ch in name
        (put chars ch true)))
    (let* ([sorted-chars (->array (sort (keys chars)))]
           [char->id @{}]
           [id->char @{}])
      (var idx 0)
      (each ch in sorted-chars
        (put char->id ch idx)
        (put id->char idx ch)
        (assign idx (inc idx)))
      (put id->char idx "<BOS>")
      @{:char->id char->id :id->char id->char
        :vocab-size (inc idx) :bos idx})))

(defn tokenize [name tokenizer]
  "Tokenize a name into array of integer IDs. Prepends and appends BOS."
  (let* ([char->id tokenizer:char->id]
         [bos tokenizer:bos]
         [ids @[bos]])
    (each ch in name
      (push ids (char->id ch)))
    (push ids bos)
    ids))

# ── Adam optimizer ──────────────────────────────────────────────

(defn make-adam [params lr beta1 beta2 eps]
  "Create Adam optimizer state."
  (let* ([n (length params)])
    @{:params params :lr lr :beta1 beta1 :beta2 beta2 :eps eps
      :m (array/new n 0.0) :v (array/new n 0.0) :step 0}))

(defn adam-step [opt lr-current]
  "One Adam update step."
  (let* ([params opt:params]
         [beta1 opt:beta1] [beta2 opt:beta2] [eps opt:eps]
         [m-arr opt:m] [v-arr opt:v]
         [step (inc opt:step)])
    (put opt :step step)
    (var i 0)
    (while (< i (length params))
      (let* ([p (params i)]
             [g (v-grad p)]
             [m-new (+ (* beta1 (m-arr i)) (* (- 1.0 beta1) g))]
             [v-new (+ (* beta2 (v-arr i)) (* (- 1.0 beta2) (* g g)))]
             [m-hat (/ m-new (- 1.0 (pow beta1 (float step))))]
             [v-hat (/ v-new (- 1.0 (pow beta2 (float step))))])
        (put m-arr i m-new)
        (put v-arr i v-new)
        (put p :data (- (v-data p) (* lr-current (/ m-hat (+ (sqrt v-hat) eps))))))
      (assign i (inc i)))))

# ── Training ────────────────────────────────────────────────────

(defn zero-grads [params]
  "Zero all gradients."
  (each p in params
    (put p :grad 0.0)))

(defn train [model tokenizer names num-steps lr]
  "Train the model."
  (let* ([params (collect-params model)]
         [opt (make-adam params lr 0.85 0.99 0.00000001)]
         [n-names (length names)]
         [max-len (inc *block-size*)])
    (println "Parameters: " (length params))
    (var step 0)
    (while (< step num-steps)
      (let* ([name (names (mod step n-names))]
             [tokens (tokenize name tokenizer)]
             [tokens (if (> (length tokens) max-len)
                       (slice tokens 0 max-len)
                       tokens)]
             [loss (cross-entropy-loss-incremental model tokens)]
             [lr-current (* lr (- 1.0 (/ (float step) (float num-steps))))])
        (backward loss)
        (adam-step opt lr-current)
        (zero-grads params)
        (when (= (mod step 100) 0)
          (println (string/format "step {:>4d} / {} | loss {:.4f}"
                                  step num-steps (v-data loss)))))
      (assign step (inc step)))))

# ── Inference ───────────────────────────────────────────────────

(defn softmax-floats [scores]
  "Numerically stable softmax over an array of floats. Returns unnormalized exps."
  (var max-val (scores 0))
  (each s in scores
    (when (> s max-val) (assign max-val s)))
  (let* ([exps @[]])
    (each s in scores
      (push exps (exp (- s max-val))))
    exps))

(defn sample-token [logits temperature]
  "Sample next token from logits using temperature-scaled softmax."
  (let* ([scaled @[]])
    (each l in logits
      (push scaled (/ (v-data l) temperature)))
    (let* ([exps (softmax-floats scaled)]
           [indices (->array (range (length exps)))])
      (rng:weighted indices exps))))

(defn generate [model tokenizer n-samples temperature max-len]
  "Generate n-samples names using incremental forward pass."
  (let* ([id->char tokenizer:id->char]
         [bos tokenizer:bos])
    (repeat n-samples
      (let* ([chars @[]]
             [[kv-keys kv-values] (make-kv-caches *n-layer*)])
        (var token-id bos)
        (var pos 0)
        (block :gen
          (while (< pos max-len)
            (let* ([logits (gpt-forward-token token-id pos kv-keys kv-values model)]
                   [next-tok (sample-token logits temperature)])
              (when (= next-tok bos) (break :gen))
              (assign token-id next-tok)
              (push chars (id->char next-tok))
              (assign pos (inc pos)))))
        (println " " (string/join chars ""))))))

# ── Gradient check ──────────────────────────────────────────────

(defn check-grads []
  "Quick gradient correctness check."
  (let* ([a (make-value 3.0)]
         [b (make-value 4.0)]
         [c (v+ (v* a b) (vpow a 2.0))])
    (backward c)
    # dc/da = b + 2a = 4 + 6 = 10
    # dc/db = a = 3
    (when (> (abs (- (v-grad a) 10.0)) 0.000001)
      (error (string/format "grad check failed: da = {} (expected 10.0)" (v-grad a))))
    (when (> (abs (- (v-grad b) 3.0)) 0.000001)
      (error (string/format "grad check failed: db = {} (expected 3.0)" (v-grad b))))
    (println "Gradient check passed.")))

# ── Main ────────────────────────────────────────────────────────

(defn main []
  (rng:seed 42)
  (check-grads)

  (println "Loading data...")
  (let* ([names (rng:shuffle (load-data "demos/microgpt/input.txt"))])
    (println "Loaded " (length names) " names")

    (let* ([tokenizer (build-tokenizer names)])
      (println "Vocab size: " tokenizer:vocab-size)

      (println "Initializing model...")
      (let* ([model (init-model tokenizer:vocab-size)])

        (println "Training...")
        (let* ([start (clock/monotonic)])
          (train model tokenizer names 1000 0.01)
          (println (string/format "Training took {:.1f}s"
                                  (- (clock/monotonic) start))))

        (println "\nGenerated names:")
        (generate model tokenizer 20 0.5 16)))))

(main)
