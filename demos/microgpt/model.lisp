# GPT model: initialization, forward pass, loss
#
# Architecture: GPT-2 style transformer (RMSNorm, no biases, ReLU instead of GeLU).
# Single-layer, 4-head attention with KV cache for incremental inference.
# Hyperparameters match the Python reference (microgpt.py).

# Hyperparameters — match Python reference exactly
(def *n-embd* 16)       # embedding dimension (width of the network)
(def *n-head* 4)        # number of attention heads
(def *head-dim* 4)      # per-head dimension (n-embd / n-head)
(def *n-layer* 1)       # number of transformer layers
(def *block-size* 16)   # maximum context length
(def *mlp-hidden* 64)   # MLP hidden dimension (4 * n-embd)
(def *eps* 0.00000001)  # epsilon for RMS normalization

# Parameter initialization

(defn init-weight [rows cols scale]
    "Create a rows x cols 2D array of Value nodes with uniform random init."
    (make-2d rows cols
       (fn [r c] (make-value (- (* (random/float) 2.0 scale) scale)))))

(defn layer-key [i suffix]
  (string/format "layer{}.{}" i suffix))

(defn init-model [vocab-size]
  "Initialize all model parameters. Returns a table of named weight matrices."
  (let* ([scale (/ 1.0 (sqrt (float *n-embd*)))]
         [model @{:wte (init-weight vocab-size *n-embd* scale)
                  :wpe (init-weight *block-size* *n-embd* scale)
                  :lm-head (init-weight vocab-size *n-embd* scale)}])
    (var layer 0)
    (while (< layer *n-layer*)
      (put model (layer-key layer "attn-wq") (init-weight *n-embd* *n-embd* scale))
      (put model (layer-key layer "attn-wk") (init-weight *n-embd* *n-embd* scale))
      (put model (layer-key layer "attn-wv") (init-weight *n-embd* *n-embd* scale))
      (put model (layer-key layer "attn-wo") (init-weight *n-embd* *n-embd* scale))
      (put model (layer-key layer "mlp-fc1") (init-weight *mlp-hidden* *n-embd* scale))
      (put model (layer-key layer "mlp-fc2") (init-weight *n-embd* *mlp-hidden* scale))
      (assign layer (+ layer 1)))
    model))

# Collect all parameters into a flat array

(defn collect-params [model]
  "Collect all Value parameter nodes from the model into a flat array."
  (let* ([params @[]])
    (each key in (keys model)
      (each row in (get model key)
        (each val in row
          (push params val))))
    params))

# Forward pass building blocks

(defn mat-vec-mul [mat vec-in]
  "Matrix-vector multiply: mat (2D array of Values) × vec-in (1D array of Values)."
  (let* ([result @[]])
    (each row in mat
      (let* ([acc (make-value 0.0)])
        (var c 0)
        (while (< c (length row))
           (assign acc (v+ acc (v* (get row c) (get vec-in c))))
           (assign c (+ c 1)))
        (push result acc)))
    result))

(defn vec-add [a b]
  "Element-wise autograd addition of two vectors."
  (array-map2 v+ a b))

(defn rms-norm [vec-in]
  "RMS normalization: x / sqrt(mean(x^2) + eps)."
  (let* ([n (length vec-in)]
         [sum-sq (make-value 0.0)])
     (each v in vec-in
       (assign sum-sq (v+ sum-sq (v* v v))))
    (let* ([mean-sq (v*s sum-sq (/ 1.0 n))]
           [rms (vpow (v+s mean-sq *eps*) 0.5)]
           [result @[]])
      (each v in vec-in
        (push result (v/ v rms)))
      result)))

(defn softmax-values [scores]
  "Softmax over an array of Value nodes. Returns array of Value nodes."
  (var max-val (v-data (get scores 0)))
  (var i 1)
  (while (< i (length scores))
    (let* ([d (v-data (get scores i))])
       (when (> d max-val) (assign max-val d)))
     (assign i (+ i 1)))
  (let* ([exps @[]]
         [sum-exp (make-value 0.0)])
    (each s in scores
      (let* ([e (vexp (v+s s (- 0.0 max-val)))])
        (push exps e)
         (assign sum-exp (v+ sum-exp e))))
    (let* ([result @[]])
      (each e in exps
        (push result (v/ e sum-exp)))
      result)))

(defn layer-weights [model i]
  "Get all weight matrices for transformer layer i."
  @{:wq  (get model (layer-key i "attn-wq"))
    :wk  (get model (layer-key i "attn-wk"))
    :wv  (get model (layer-key i "attn-wv"))
    :wo  (get model (layer-key i "attn-wo"))
    :fc1 (get model (layer-key i "mlp-fc1"))
    :fc2 (get model (layer-key i "mlp-fc2"))})

# Per-token forward pass (incremental, with KV cache)
# This matches the original CL implementation: process one token at a time,
# accumulating key/value vectors in per-layer caches.

(defn gpt-forward-token [token-id pos-id kv-keys kv-values model]
  "Forward pass for a single token at position pos-id.
   kv-keys and kv-values are arrays of length n-layer, each containing
   an array of past key/value vectors (one per previous position).
   Mutates kv-keys and kv-values by appending new k/v vectors.
   Returns a 1D array of logit Value nodes."
  (let* ([wte (get model :wte)]
         [wpe (get model :wpe)]
         [lm-head (get model :lm-head)]
         [tok-emb (get wte token-id)]
         [pos-emb (get wpe pos-id)]
         [x (vec-add tok-emb pos-emb)])
     (assign x (rms-norm x))
    # Transformer layers
    (var li 0)
    (while (< li *n-layer*)
      (let* ([weights (layer-weights model li)]
             [wq (get weights :wq)]
             [wk (get weights :wk)]
             [wv (get weights :wv)]
             [wo (get weights :wo)]
             [fc1 (get weights :fc1)]
             [fc2 (get weights :fc2)]
             [x-residual x])
        # Pre-norm + Q/K/V projections
        (assign x (rms-norm x))
        (let* ([q (mat-vec-mul wq x)]
               [k (mat-vec-mul wk x)]
               [v (mat-vec-mul wv x)])
          # Append k, v to caches
          (push (get kv-keys li) k)
          (push (get kv-values li) v)
          (let* ([layer-keys (get kv-keys li)]
                 [layer-vals (get kv-values li)]
                 [n-t (length layer-keys)]
                 [x-attn (array/new *n-embd* (make-value 0.0))])
            # Multi-head attention
            (var h 0)
            (while (< h *n-head*)
              (let* ([hs (* h *head-dim*)]
                     [q-head (slice q hs (+ hs *head-dim*))]
                     [scale-factor (/ 1.0 (sqrt (float *head-dim*)))]
                     [attn-logits @[]])
                # Dot product of q with each past key
                (var t-idx 0)
                (while (< t-idx n-t)
                  (let* ([k-t (get layer-keys t-idx)]
                         [dot (make-value 0.0)])
                    (var d 0)
                    (while (< d *head-dim*)
                       (assign dot (v+ dot (v* (get q-head d) (get k-t (+ hs d)))))
                       (assign d (+ d 1)))
                    (push attn-logits (v*s dot scale-factor)))
                   (assign t-idx (+ t-idx 1)))
                # Softmax
                (let* ([attn-weights (softmax-values attn-logits)])
                  # Weighted sum of values
                  (var j 0)
                  (while (< j *head-dim*)
                    (let* ([acc (v* (get attn-weights 0)
                                    (get (get layer-vals 0) (+ hs j)))])
                      (var t-idx2 1)
                      (while (< t-idx2 n-t)
                         (assign acc (v+ acc (v* (get attn-weights t-idx2)
                                              (get (get layer-vals t-idx2) (+ hs j)))))
                         (assign t-idx2 (+ t-idx2 1)))
                      (put x-attn (+ hs j) acc))
                     (assign j (+ j 1)))))
               (assign h (+ h 1)))
            # Project attention output
            (assign x (mat-vec-mul wo x-attn))
            # Residual
            (assign x (vec-add x x-residual))))
        # MLP block
        (let* ([x-residual2 x])
           (assign x (rms-norm x))
           (assign x (mat-vec-mul fc1 x))
           (assign x (array-map vrelu x))
           (assign x (mat-vec-mul fc2 x))
           (assign x (vec-add x x-residual2))))
       (assign li (+ li 1)))
    # Project to vocab
    (mat-vec-mul lm-head x)))

# Loss

(defn cross-entropy-loss-incremental [model tokens]
  "Compute cross-entropy loss over a token sequence using incremental forward.
   tokens includes BOS at start and end.
   Returns a single Value node (the mean loss)."
  (let* ([n (min *block-size* (- (length tokens) 1))]
         [[kv-keys kv-values] (make-kv-caches *n-layer*)])
    # Accumulate loss over positions
    (let* ([total-loss (make-value 0.0)])
      (var pos 0)
      (while (< pos n)
        (let* ([token-id (get tokens pos)]
               [target-id (get tokens (+ pos 1))]
               [logits (gpt-forward-token token-id pos kv-keys kv-values model)]
               [probs (softmax-values logits)]
               [loss-t (vneg (vlog (get probs target-id)))])
           (assign total-loss (v+ total-loss loss-t)))
         (assign pos (+ pos 1)))
      (v*s total-loss (/ 1.0 (float n))))))
