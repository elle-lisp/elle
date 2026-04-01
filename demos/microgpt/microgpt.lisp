#!/usr/bin/env elle
(elle/epoch 6)
# ── microgpt: minimal GPT in Elle ───────────────────────────────
#
# Port of https://github.com/karpathy/microgpt
# Scalar autograd + character-level GPT trained on names.
#
# Usage: cargo run --release -- demos/microgpt/microgpt.lisp

(import "plugin/random")
(import "demos/microgpt/helpers.lisp")
(import "demos/microgpt/autograd.lisp")
(import "demos/microgpt/model.lisp")

# ── Data loading and tokenizer ──────────────────────────────────

(defn load-data [path]
  "Load names from file, return mutable array of trimmed strings.
   Preserves original case to match Python reference (uppercase initials
   are distinct tokens from their lowercase counterparts)."
  (let* ([lines (read-lines path)]
         [result @[]])
    (each line in lines
      (let* ([trimmed (string/trim line)])
        (when (> (length trimmed) 0)
          (push result trimmed))))
    result))

(defn sort-strings [arr]
  "Insertion sort for an array of strings. Returns a new sorted array.
   Used to produce a deterministic char->id mapping matching the Python reference."
  (let* ([result @[]])
    (each s in arr
      (push result s))
    (var i 1)
    (while (< i (length result))
      (let* ([key (result i)])
        (var j (- i 1))
        (while (and (>= j 0) (> (result j) key))
          (put result (+ j 1) (result j))
          (assign j (- j 1)))
        (put result (+ j 1) key))
      (assign i (+ i 1)))
    result))

(defn build-tokenizer [names]
  "Build char-level tokenizer. Returns @struct with :char->id, :id->char, :vocab-size.
   BOS token is at the last index (also serves as EOS).
   Characters are sorted to produce a deterministic mapping matching Python."
  (let* ([chars @{}])
    # Collect unique chars from all names
    (each name in names
      (var i 0)
      (while (< i (length name))
        (put chars (name i) true)
        (assign i (+ i 1))))
    (let* ([sorted-chars (sort-strings (keys chars))]
           [char->id @{}]
           [id->char @{}])
      (var idx 0)
      (each ch in sorted-chars
        (put char->id ch idx)
        (put id->char idx ch)
        (assign idx (+ idx 1)))
      # BOS/EOS is the last index
      (let* ([bos idx])
        (put id->char bos "<BOS>")
        @{:char->id char->id
          :id->char id->char
          :vocab-size (+ idx 1)
          :bos bos}))))

(defn tokenize [name tokenizer]
  "Tokenize a name into array of integer IDs. Prepends and appends BOS."
  (let* ([char->id tokenizer:char->id]
         [bos tokenizer:bos]
         [ids @[bos]])
    (var i 0)
    (while (< i (length name))
      (push ids (char->id (name i)))
      (assign i (+ i 1)))
    (push ids bos)
    ids))

# ── Adam optimizer ──────────────────────────────────────────────

(defn make-adam [params lr beta1 beta2 eps]
  "Create Adam optimizer state."
  (let* ([n (length params)]
         [m (@array/new n 0.0)]
         [v (@array/new n 0.0)])
    @{:params params :lr lr :beta1 beta1 :beta2 beta2 :eps eps
      :m m :v v :step 0}))

(defn adam-step [opt lr-current]
  "One Adam update step."
  (let* ([params opt:params]
         [beta1 opt:beta1]
         [beta2 opt:beta2]
         [eps opt:eps]
         [m-arr opt:m]
         [v-arr opt:v]
         [step (+ opt:step 1)])
    (put opt :step step)
    (var i 0)
    (while (< i (length params))
      (let* ([p (params i)]
             [g (v-grad p)]
             [m-old (m-arr i)]
             [v-old (v-arr i)]
             [m-new (+ (* beta1 m-old) (* (- 1.0 beta1) g))]
             [v-new (+ (* beta2 v-old) (* (- 1.0 beta2) (* g g)))]
             [m-hat (/ m-new (- 1.0 (pow beta1 (float step))))]
             [v-hat (/ v-new (- 1.0 (pow beta2 (float step))))])
        (put m-arr i m-new)
        (put v-arr i v-new)
        (put p :data (- (v-data p) (* lr-current (/ m-hat (+ (sqrt v-hat) eps))))))
      (assign i (+ i 1)))))

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
         [max-len (+ *block-size* 1)])
    (println "Parameters: " (length params))
    (var step 0)
    (while (< step num-steps)
      # Pick a random name
      (let* ([idx (floor (* (random/float) n-names))]
             [name (names idx)]
             [tokens (tokenize name tokenizer)]
             [tokens (if (> (length tokens) max-len)
                       (slice tokens 0 max-len)
                       tokens)])
        # Forward + loss (incremental per-token)
        (let* ([loss (cross-entropy-loss-incremental model tokens)]
               [lr-current (* lr (- 1.0 (/ (float step) (float num-steps))))])
          # Backward
          (backward! loss)
          # Update
          (adam-step opt lr-current)
          # Zero grads
          (zero-grads params)
          # Log
          (when (= (mod step 100) 0)
            (println (string/format "step {:>4d} / {} | loss {:.4f}"
                                    step num-steps (v-data loss))))))
      (assign step (+ step 1)))))

# ── Inference ───────────────────────────────────────────────────

(defn softmax-floats [scores]
  "Numerically stable softmax over an array of floats. Returns [probs sum]."
  (var max-val (scores 0))
  (each s in scores
    (when (> s max-val) (assign max-val s)))
  (let* ([exps @[]]
         [sum-exp 0.0])
    (each s in scores
      (let* ([e (exp (- s max-val))])
        (push exps e)
        (assign sum-exp (+ sum-exp e))))
    [exps sum-exp]))

(defn sample-token [logits temperature]
  "Sample next token from logits using temperature-scaled softmax."
  (let* ([scaled @[]])
    (each l in logits
      (push scaled (/ (v-data l) temperature)))
    (let* ([[exps sum-exp] (softmax-floats scaled)]
           [r (random/float)])
      (var cumulative 0.0)
      (var idx 0)
      (block :sample
        (while (< idx (length exps))
          (assign cumulative (+ cumulative (/ (exps idx) sum-exp)))
          (when (>= cumulative r) (break :sample idx))
          (assign idx (+ idx 1)))
        (- (length exps) 1)))))

(defn generate [model tokenizer n-samples temperature max-len]
  "Generate n-samples names using incremental forward pass."
  (let* ([id->char tokenizer:id->char]
         [bos tokenizer:bos])
    (var sample 0)
    (while (< sample n-samples)
      (let* ([name ""]
             [[kv-keys kv-values] (make-kv-caches *n-layer*)])
        (var token-id bos)
        (var done false)
        (var pos 0)
        (while (and (not done) (< pos max-len))
          (let* ([logits (gpt-forward-token token-id pos kv-keys kv-values model)]
                 [next-tok (sample-token logits temperature)])
            (if (= next-tok bos)
              (assign done true)
              (begin
                (assign token-id next-tok)
                (assign name (string name (id->char next-tok)))
                (assign pos (+ pos 1))))))
        (println " " name))
      (assign sample (+ sample 1)))))

# ── Gradient check ──────────────────────────────────────────────

(defn check-grads []
  "Quick gradient correctness check."
  (let* ([a (make-value 3.0)]
         [b (make-value 4.0)]
         [c (v+ (v* a b) (vpow a 2.0))])
    (backward! c)
    # dc/da = b + 2a = 4 + 6 = 10
    # dc/db = a = 3
    (when (> (abs (- (v-grad a) 10.0)) 0.000001)
      (error (string/format "grad check failed: da = {} (expected 10.0)" (v-grad a))))
    (when (> (abs (- (v-grad b) 3.0)) 0.000001)
      (error (string/format "grad check failed: db = {} (expected 3.0)" (v-grad b))))
    (println "Gradient check passed.")))

# ── Main ────────────────────────────────────────────────────────

(defn main []
  (random/seed 42)

  # Verify autograd
  (check-grads)

  # Load data
  (println "Loading data...")
  (let* ([names (load-data "demos/microgpt/input.txt")])
    (shuffle! names)
    (println "Loaded " (length names) " names")

    # Build tokenizer
    (let* ([tokenizer (build-tokenizer names)])
      (println "Vocab size: " tokenizer:vocab-size)

      # Initialize model
      (println "Initializing model...")
      (let* ([model (init-model tokenizer:vocab-size)])

        # Train
        (println "Training...")
        (let* ([start (clock/monotonic)])
          (train model tokenizer names 1000 0.01)
          (let* ([elapsed (- (clock/monotonic) start)])
            (println (string/format "Training took {:.1f}s" elapsed))))

        # Generate
        (println "\nGenerated names:")
        (generate model tokenizer 20 0.5 16)))))

(main)
