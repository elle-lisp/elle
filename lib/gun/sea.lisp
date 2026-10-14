(elle/epoch 15)
# audited: 2026-10-14
## The SEA rules of std/gun: key pairs, user spaces, the signature of a field, the content node, admission, and encryption.
## lib/gun/sea.md
## lib/gun/overview.md
##
## Loaded via:
##   (def sea ((import "std/gun/sea") :value value))
##   (def sea ((import "std/gun/sea") :value value
##                                    :crypto (import "plugin/crypto")
##                                    :random (import "plugin/random")))
##
## Exports: {:space-of :content-soul :check-soul :content-key :pair :sign
##           :admit :admitted :secret :encrypt :decrypt}
##
## Without both plugins, pair, secret, encrypt and decrypt raise
## :sea-not-configured, and so does each check of a soul in a space. Without
## :crypto, content-key and each check of the content node raise it.

(fn [&named value crypto random]

  (def field ((import "std/gun/field")))

  (def key-size 32)
  (def signature-size 64)
  (def nonce-size 24)
  (def tag-size 16)

  (defn gun-error [reason message &named soul key]
    (error {:error :gun-error
            :reason reason
            :soul soul
            :key key
            :message message}))

  (defn require-sea []
    (when (or (nil? crypto) (nil? random))
      (gun-error :sea-not-configured ":crypto and :random were not both supplied to (import \"std/gun\")")))

  (defn require-hash []
    (when (nil? crypto)
      (gun-error :sea-not-configured ":crypto was not supplied to (import \"std/gun\")")))

  (defn hex->bytes [h]
    "The bytes that the lowercase hex h spells. Raises :bad-encoding on
     anything else."
    (value:decode (string "x:" h)))

  (defn derive [seed info]
    "The 32-byte key that HKDF-SHA256 derives from seed under info."
    (crypto:hkdf-sha256 seed "" info key-size))

  ## ── User spaces ──────────────────────────────────────────────────────

  (defn space-of [soul]
    "The space soul belongs to: its text up to the first /, when it starts
     with ~. nil for a soul outside every space."
    (when (and (string? soul) (string/starts-with? soul "~"))
      (let [slash (string/find soul "/")]
        (if (nil? slash) soul (slice soul 0 slash)))))

  ## ── The content node ─────────────────────────────────────────────────

  (def content-soul "#")

  (defn content-key [v]
    "The key of v in the content node: the lowercase hex of the SHA-256 of
     v's encoding. Raises :invalid-value for what is not a value."
    (require-hash)
    (seq->hex (crypto:sha256 (value:encode v))))

  ## ── Checking a soul ──────────────────────────────────────────────────

  (defn check-soul [soul]
    "How admission checks the fields of soul: the public key, as bytes, that
     signs a soul in a space, :hash for the content node, and nil for any
     other soul. Raises :sea-not-configured when the check needs a plugin
     this module lacks. Raises :invalid-space for a space whose name is not
     ~ and 64 lowercase hex characters, and for any other soul that starts
     with #."
    (let [space (space-of soul)]
      (cond
        (= soul content-soul) (begin
                                (require-hash)
                                :hash)
        (string/starts-with? soul content-soul) (gun-error :invalid-space (string "the one soul that starts with # is #: "
        soul) :soul soul)
        (nil? space) nil
        (begin
          (require-sea)
          (let [[ok? pub] (protect (hex->bytes (slice space 1)))]
            (unless (and ok? (= (length pub) key-size))
              (gun-error :invalid-space (string "a space is ~ and 64 lowercase hex characters: "
                         space) :soul soul))
            pub)))))

  ## ── Pairs ────────────────────────────────────────────────────────────

  (defn pair [&opt seed]
    "The key pair of the 32-byte seed, or of a fresh random seed: a struct of
     :seed, and of :pub and :epub, the public keys in hex."
    (require-sea)
    (let [s (if (nil? seed) (random:csprng-bytes key-size) seed)]
      (unless (and (bytes? s) (= (length s) key-size))
        (gun-error :bad-seed "a seed is 32 bytes"))
      {:seed s
       :pub (seq->hex (crypto:ed25519-public (derive s "gun/sea/sign")))
       :epub (seq->hex (crypto:x25519-public (derive s "gun/sea/encrypt")))}))

  ## ── Signatures ───────────────────────────────────────────────────────

  (defn signed-text [soul key f]
    "The text that the signature of field f of key in soul covers."
    (json/serialize ["gun/sea/sign" soul key (field:state f)
                     (value:encode (field:value f))]))

  (defn sign [signer writes]
    "writes, an array of [soul fields], with each field in the space of the
     pair signer signed by it. A field in another space or in none stays as
     it was, and so does every field when signer is nil."
    (if (nil? signer)
      writes
      (let [space (string "~" signer:pub)
            seed (derive signer:seed "gun/sea/sign")]
        (map (fn [[soul fields]]
               (if (= (space-of soul) space)
                 (let [signed @{}]
                   (each [key f] in fields
                     (put signed key
                          (field:make (field:state f) (field:value f)
                                      (crypto:ed25519-sign seed
                                      (signed-text soul key f)))))
                   [soul (freeze signed)])
                 [soul fields])) writes))))

  ## ── Admission ────────────────────────────────────────────────────────

  (defn verdict [soul key f check]
    "The reason field f of key in soul fails admission under check, what
     check-soul answers for soul, or nil when it passes. Outside every space,
     a field passes only without a signature, and in the content node, only
     under the key of its value."
    (let [sig (field:signature f)]
      (cond
        (= check :hash)
          (cond
            sig :bad-signature
            (= key (content-key (field:value f))) nil
            :bad-hash)
        (nil? check) (when sig :bad-signature)
        (nil? sig) :unsigned
        (not (and (bytes? sig) (= (length sig) signature-size))) :bad-signature
        (crypto:ed25519-verify check (signed-text soul key f) sig) nil
        :bad-signature)))

  (defn refusal [reason soul key]
    {:error :gun-error
     :reason reason
     :soul soul
     :key key
     :message (string "field " key " of " soul " fails admission: " reason)})

  (defn admit [writes]
    "Raise the refusal of the first field in writes, pairs of soul and
     fields, that fails admission. Checks every soul before any field, so a
     soul that may not exist raises before a field that names it."
    (each [soul _] in writes
      (check-soul soul))
    (each [soul fields] in writes
      (let [check (check-soul soul)]
        (each [key f] in fields
          (when-let [r (verdict soul key f check)] (error (refusal r soul key)))))))

  (defn admitted [soul fields report]
    "The fields of soul that pass admission. Calls report with the refusal
     of each field that fails."
    (let [check (check-soul soul)
          out @{}]
      (each [key f] in fields
        (let [r (verdict soul key f check)]
          (if r (report (refusal r soul key)) (put out key f))))
      (freeze out)))

  ## ── Encryption ───────────────────────────────────────────────────────

  (def encrypt-aad "gun/sea/encrypt")

  (defn secret [p epub]
    "The 32-byte key that the pair p shares with the owner of epub, the hex
     of an X25519 public key."
    (require-sea)
    (let [[hex-ok? theirs] (protect (hex->bytes epub))]
      (unless (and hex-ok? (= (length theirs) key-size))
        (gun-error :bad-public-key "an epub is 64 lowercase hex characters"))
      (let [[ok? shared] (protect (crypto:x25519 (derive p:seed
                                  "gun/sea/encrypt") theirs))]
        (unless ok? (gun-error :bad-public-key "the epub has small order"))
        (crypto:hkdf-sha256 shared "" "gun/sea/secret" key-size))))

  (defn require-key [k]
    (unless (and (bytes? k) (= (length k) key-size))
      (gun-error :bad-key "a key is 32 bytes")))

  (defn encrypt [k v]
    "The value v sealed under the 32-byte key k: a random nonce, then the
     ciphertext of v's encoding and its tag."
    (require-sea)
    (require-key k)
    (let [nonce (random:csprng-bytes nonce-size)]
      (concat nonce
              (crypto:xchacha20poly1305-seal k nonce (value:encode v)
              encrypt-aad))))

  (defn decrypt [k b]
    "The value that encrypt sealed as b under the key k. Raises
     :decrypt-failed when b does not open under k."
    (require-sea)
    (require-key k)
    (let [failed (fn []
                   (gun-error :decrypt-failed "the bytes do not open under the key"))]
      (unless (and (bytes? b) (>= (length b) (+ nonce-size tag-size))) (failed))
      (let [plain (crypto:xchacha20poly1305-open k (slice b 0 nonce-size)
            (slice b nonce-size) encrypt-aad)]
        (when (nil? plain) (failed))
        (let [[ok? v] (protect (value:decode (string plain)))]
          (unless ok? (failed))
          v))))

  {:space-of space-of
   :content-soul content-soul
   :check-soul check-soul
   :content-key content-key
   :pair pair
   :sign sign
   :admit admit
   :admitted admitted
   :secret secret
   :encrypt encrypt
   :decrypt decrypt})
