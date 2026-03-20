(elle/epoch 1)

## Crypto plugin integration tests
## Tests the crypto plugin (.so loaded via import-file)

## Try to load the crypto plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_crypto.so")))
(when (not ok?)
  (display "SKIP: crypto plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def sha256-fn      (get plugin :sha256))
(def hmac-sha256-fn (get plugin :hmac-sha256))

## ── crypto/sha256 ──────────────────────────────────────────────

(assert (= (bytes->hex (sha256-fn "")) "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855") "sha256 empty string")

(assert (= (bytes->hex (sha256-fn "hello")) "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824") "sha256 hello")

## ── crypto/hmac-sha256 ────────────────────────────────────────

(assert (= (bytes->hex (hmac-sha256-fn "key" "message")) "6e9ef29b75fffc5b7abae527d58fdadb2fe42e7219011976917343065f58ed4a") "hmac-sha256 key message")
