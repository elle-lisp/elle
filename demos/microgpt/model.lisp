(elle/epoch 9)
## model.lisp — GPT model: initialization, forward pass, loss
##
## Architecture: GPT-2 style transformer (RMSNorm, no biases, ReLU
## instead of GeLU). Single-layer, 4-head attention with KV cache
## for incremental inference.

(fn [ag helpers rng]

  # ── Hyperparameters ────────────────────────────────────────────

  (def *n-embd* 16)
  (def *n-head* 4)
  (def *head-dim* 4)
  (def *n-layer* 1)
  (def *block-size* 16)
  (def *mlp-hidden* 64)
  (def *eps* 0.00000001)

  # ── Parameter initialization ───────────────────────────────────

  (defn init-weight [rows cols scale]
    "Create a rows x cols 2D array of Value nodes with Gaussian random init."
    (helpers:make-2d rows cols (fn [r c] (ag:make-value (rng:normal 0.0 scale)))))
  (defn layer-key [i suffix]
    (string/format "layer{}.{}" i suffix))
  (defn init-model [vocab-size]
    "Initialize all model parameters. Returns an @struct of named weight matrices."
    (let [scale (/ 1.0 (sqrt (float *n-embd*)))]
      (let [model @{:wte (init-weight vocab-size *n-embd* scale)
                    :wpe (init-weight *block-size* *n-embd* scale)
                    :lm-head (init-weight vocab-size *n-embd* scale)}]
        (each layer in (range *n-layer*)
          (put model (layer-key layer "attn-wq")
            (init-weight *n-embd* *n-embd* scale))
          (put model (layer-key layer "attn-wk")
            (init-weight *n-embd* *n-embd* scale))
          (put model (layer-key layer "attn-wv")
            (init-weight *n-embd* *n-embd* scale))
          (put model (layer-key layer "attn-wo")
            (init-weight *n-embd* *n-embd* scale))
          (put model (layer-key layer "mlp-fc1")
            (init-weight *mlp-hidden* *n-embd* scale))
          (put model (layer-key layer "mlp-fc2")
            (init-weight *n-embd* *mlp-hidden* scale)))
        model)))

  # ── Collect parameters ─────────────────────────────────────────

  (defn collect-params [model]
    "Collect all Value parameter nodes from the model into a flat array."
    (let [params @[]]
      (each key in (keys model)
        (each row in (model key)
          (each val in row
            (push params val))))
      params))

  # ── Forward pass building blocks ───────────────────────────────

  (defn mat-vec-mul [mat vec-in]
    "Matrix-vector multiply: mat[rows x cols] * vec-in[cols] → result[rows].
     Uses fused dot product — one Value node per output element."
    (let [n (length vec-in)
          result @[]]
      (each row in mat
        (push result (ag:vdot row vec-in n)))
      result))
  (defn vec-add [a b]
    "Element-wise autograd addition of two vectors."
    (let [result @[]]
      (def @i 0)
      (while (< i (length a))
        (push result (ag:v+ (a i) (b i)))
        (assign i (inc i)))
      result))
  (defn rms-norm [vec-in]
    "RMS normalization: x / sqrt(mean(x^2) + eps)."
    (let* [squares (map (fn [v] (ag:v* v v)) vec-in)
           sum-sq (ag:vsum squares)
           rms (ag:vpow (ag:v+s (ag:v*s sum-sq (/ 1.0 (length vec-in))) *eps*)
             0.5)]
      (thaw (->array (map (fn [v] (ag:v/ v rms)) vec-in)))))
  (defn softmax-values [scores]
    "Softmax over an array of Value nodes."
    (def @max-val (ag:v-data (scores 0)))
    (each s in scores
      (when (> (ag:v-data s) max-val) (assign max-val (ag:v-data s))))
    (def @sum-exp (ag:make-value 0.0))
    (let [exps (->array (map (fn [s] (ag:vexp (ag:v+s s (- 0.0 max-val))))
                          scores))]
      (each e in exps
        (assign sum-exp (ag:v+ sum-exp e)))
      (thaw (->array (map (fn [e] (ag:v/ e sum-exp)) exps)))))
  (defn layer-weights [model i]
    "Get all weight matrices for transformer layer i."
    {:wq (model (layer-key i "attn-wq"))
     :wk (model (layer-key i "attn-wk"))
     :wv (model (layer-key i "attn-wv"))
     :wo (model (layer-key i "attn-wo"))
     :fc1 (model (layer-key i "mlp-fc1"))
     :fc2 (model (layer-key i "mlp-fc2"))})

  # ── Per-token forward pass ─────────────────────────────────────

  (defn attn-head [h q layer-keys layer-vals n-t x-attn]
    "Compute one attention head and write results into x-attn."
    (let* [hs (* h *head-dim*)
           sf (/ 1.0 (sqrt (float *head-dim*)))
           attn-logits @[]]
      (def @ti 0)
      (while (< ti n-t)
        (push attn-logits
          (ag:v*s (ag:vdot q (layer-keys ti) *head-dim* :offset-a hs
              :offset-b hs) sf))
        (assign ti (inc ti)))  # Weighted sum of cached values
      (let [aw (softmax-values attn-logits)]
        (each j in (range *head-dim*)
          (def @acc (ag:make-value 0.0))
          (def @t2 0)
          (while (< t2 n-t)
            (assign acc (ag:v+ acc (ag:v* (aw t2) ((layer-vals t2) (+ hs j)))))
            (assign t2 (inc t2)))
          (put x-attn (+ hs j) acc)))))
  (defn attn-block [x weights kv-keys kv-values li]
    "Multi-head attention with KV cache. Returns updated x with residual."
    (let* [x-norm (rms-norm x)
           q (mat-vec-mul weights:wq x-norm)
           k (mat-vec-mul weights:wk x-norm)
           v (mat-vec-mul weights:wv x-norm)
           layer-keys (begin
                        (push (kv-keys li) k)
                        (kv-keys li))
           layer-vals (begin
                        (push (kv-values li) v)
                        (kv-values li))
           n-t (length layer-keys)
           x-attn (array/new *n-embd* (ag:make-value 0.0))]
      (each h in (range *n-head*)
        (attn-head h q layer-keys layer-vals n-t x-attn))
      (vec-add (mat-vec-mul weights:wo x-attn) x)))
  (defn mlp-block [x weights]
    "MLP block with residual connection."
    (let* [h (mat-vec-mul weights:fc1 (rms-norm x))
           h (thaw (->array (map ag:vrelu h)))
           h (mat-vec-mul weights:fc2 h)]
      (vec-add h x)))
  (defn gpt-forward-token [token-id pos-id kv-keys kv-values model]
    "Forward pass for a single token at position pos-id.
     Mutates kv-keys and kv-values by appending new k/v vectors.
     Returns a 1D array of logit Value nodes."
    (def @x (rms-norm (vec-add (model:wte token-id) (model:wpe pos-id))))
    (each li in (range *n-layer*)
      (let [weights (layer-weights model li)]
        (assign x (attn-block x weights kv-keys kv-values li))
        (assign x (mlp-block x weights))))
    (mat-vec-mul model:lm-head x))

  # ── Loss ───────────────────────────────────────────────────────

  (defn cross-entropy-loss-incremental [model tokens]
    "Compute cross-entropy loss over a token sequence using incremental forward.
     Returns a single Value node (the mean loss)."
    (let* [n (min *block-size* (dec (length tokens)))
           [kv-keys kv-values] (helpers:make-kv-caches *n-layer*)
           @total-loss (ag:make-value 0.0)]
      (def @pos 0)
      (while (< pos n)
        (let* [logits (gpt-forward-token (tokens pos) pos kv-keys kv-values
                 model)
               probs (softmax-values logits)]
          (assign
            total-loss
            (ag:v+ total-loss (ag:vneg (ag:vlog (probs (tokens (inc pos))))))))
        (assign pos (inc pos)))
      (ag:v*s total-loss (/ 1.0 (float n)))))
  {:init-model init-model
   :collect-params collect-params
   :gpt-forward-token gpt-forward-token
   :cross-entropy-loss-incremental cross-entropy-loss-incremental
   :*block-size* *block-size*
   :*n-layer* *n-layer*})
