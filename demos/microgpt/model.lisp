# GPT model: initialization, forward pass, loss

# Hyperparameters
(def *n-embd* 8)
(def *n-head* 2)
(def *head-dim* 4)
(def *n-layer* 1)
(def *block-size* 16)
(def *mlp-hidden* 32)

# Parameter initialization

(defn init-weight [rows cols scale]
  "Create a rows x cols 2D array of Value nodes with uniform random init."
  (make-2d rows cols
    (fn [r c] (make-value (- (* (random) 2.0 scale) scale)))))

(defn init-model [vocab-size]
  "Initialize all model parameters. Returns a table of named weight matrices."
  (let* ([scale (/ 1.0 (sqrt (float *n-embd*)))]
         [model @{:wte (init-weight vocab-size *n-embd* scale)
                  :wpe (init-weight *block-size* *n-embd* scale)
                  :lm-head (init-weight vocab-size *n-embd* scale)}])
    (var layer 0)
    (while (< layer *n-layer*)
      (let* ([prefix (string/format "layer{}" layer)])
        (put model (string/format "{}.attn-wq" prefix) (init-weight *n-embd* *n-embd* scale))
        (put model (string/format "{}.attn-wk" prefix) (init-weight *n-embd* *n-embd* scale))
        (put model (string/format "{}.attn-wv" prefix) (init-weight *n-embd* *n-embd* scale))
        (put model (string/format "{}.attn-wo" prefix) (init-weight *n-embd* *n-embd* scale))
        (put model (string/format "{}.mlp-fc1" prefix) (init-weight *mlp-hidden* *n-embd* scale))
        (put model (string/format "{}.mlp-fc2" prefix) (init-weight *n-embd* *mlp-hidden* scale)))
      (set layer (+ layer 1)))
    model))

# Collect all parameters into a flat array

(defn collect-params [model]
  "Return a flat array of all Value nodes in the model."
  (let* ([params @[]])
    (each key in (keys model)
      (let* ([mat (get model key)]
             [rows (length mat)])
        (var r 0)
        (while (< r rows)
          (let* ([row (get mat r)])
            (var c 0)
            (while (< c (length row))
              (push params (get row c))
              (set c (+ c 1))))
          (set r (+ r 1)))))
    params))

# Forward pass building blocks

(defn mat-vec-mul [mat vec-in]
  "Multiply 2D weight matrix by 1D vector of Value nodes.
   mat is out_dim x in_dim, vec-in is in_dim. Returns 1D array."
  (let* ([out-dim (length mat)]
         [result @[]])
    (var r 0)
    (while (< r out-dim)
      (let* ([row (get mat r)]
             [acc (v* (get row 0) (get vec-in 0))])
        (var c 1)
        (while (< c (length row))
          (set acc (v+ acc (v* (get row c) (get vec-in c))))
          (set c (+ c 1)))
        (push result acc))
      (set r (+ r 1)))
    result))

(defn vec-map-fn [f vec-in]
  "Apply f to each element of array, returning new array."
  (let* ([result @[]])
    (each v in vec-in
      (push result (f v)))
    result))

(defn vec-add [a b]
  "Element-wise v+ of two Value-node arrays."
  (map2 v+ a b))

(defn rms-norm [vec-in]
  "RMS normalization: x / sqrt(mean(x^2) + eps)."
  (let* ([n (length vec-in)]
         [sum-sq (make-value 0.0)])
    (each v in vec-in
      (set sum-sq (v+ sum-sq (v* v v))))
    (let* ([mean-sq (v*s sum-sq (/ 1.0 n))]
           [rms (vpow (v+s mean-sq 0.00000001) 0.5)]
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
      (when (> d max-val) (set max-val d)))
    (set i (+ i 1)))
  (let* ([exps @[]]
         [sum-exp (make-value 0.0)])
    (each s in scores
      (let* ([e (vexp (v+s s (- 0.0 max-val)))])
        (push exps e)
        (set sum-exp (v+ sum-exp e))))
    (let* ([result @[]])
      (each e in exps
        (push result (v/ e sum-exp)))
      result)))

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
    (set x (rms-norm x))
    # Transformer layers
    (var li 0)
    (while (< li *n-layer*)
      (let* ([prefix (string/format "layer{}" li)]
             [wq (get model (string/format "{}.attn-wq" prefix))]
             [wk (get model (string/format "{}.attn-wk" prefix))]
             [wv (get model (string/format "{}.attn-wv" prefix))]
             [wo (get model (string/format "{}.attn-wo" prefix))]
             [fc1 (get model (string/format "{}.mlp-fc1" prefix))]
             [fc2 (get model (string/format "{}.mlp-fc2" prefix))]
             [x-residual x])
        # Pre-norm + Q/K/V projections
        (set x (rms-norm x))
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
                         [k-head (slice k-t hs (+ hs *head-dim*))]
                         [dot (make-value 0.0)])
                    (var d 0)
                    (while (< d *head-dim*)
                      (set dot (v+ dot (v* (get q-head d) (get k-head d))))
                      (set d (+ d 1)))
                    (push attn-logits (v*s dot scale-factor)))
                  (set t-idx (+ t-idx 1)))
                # Softmax
                (let* ([attn-weights (softmax-values attn-logits)])
                  # Weighted sum of values
                  (var j 0)
                  (while (< j *head-dim*)
                    (let* ([acc (v* (get attn-weights 0)
                                    (get (slice (get layer-vals 0) hs (+ hs *head-dim*)) j))])
                      (var t-idx2 1)
                      (while (< t-idx2 n-t)
                        (set acc (v+ acc (v* (get attn-weights t-idx2)
                                             (get (slice (get layer-vals t-idx2) hs (+ hs *head-dim*)) j))))
                        (set t-idx2 (+ t-idx2 1)))
                      (put x-attn (+ hs j) acc))
                    (set j (+ j 1)))))
              (set h (+ h 1)))
            # Project attention output
            (set x (mat-vec-mul wo x-attn))
            # Residual
            (set x (vec-add x x-residual))))
        # MLP block
        (let* ([x-residual2 x])
          (set x (rms-norm x))
          (set x (mat-vec-mul fc1 x))
          (set x (vec-map-fn vrelu x))
          (set x (mat-vec-mul fc2 x))
          (set x (vec-add x x-residual2))))
      (set li (+ li 1)))
    # Project to vocab
    (mat-vec-mul lm-head x)))

# Loss

(defn cross-entropy-loss-incremental [model tokens]
  "Compute cross-entropy loss over a token sequence using incremental forward.
   tokens includes BOS at start and end.
   Returns a single Value node (the mean loss)."
  (let* ([n (min *block-size* (- (length tokens) 1))]
         [kv-keys @[]]
         [kv-values @[]])
    # Initialize per-layer KV caches
    (var li 0)
    (while (< li *n-layer*)
      (push kv-keys @[])
      (push kv-values @[])
      (set li (+ li 1)))
    # Accumulate loss over positions
    (let* ([total-loss (make-value 0.0)])
      (var pos 0)
      (while (< pos n)
        (let* ([token-id (get tokens pos)]
               [target-id (get tokens (+ pos 1))]
               [logits (gpt-forward-token token-id pos kv-keys kv-values model)]
               [probs (softmax-values logits)]
               [loss-t (v-neg (vlog (get probs target-id)))])
          (set total-loss (v+ total-loss loss-t)))
        (set pos (+ pos 1)))
      (v*s total-loss (/ 1.0 (float n))))))
