# sea

<!-- audited: 2026-10-12 -->

The SEA rules of std/gun: key pairs, user spaces that only their key writes, the signed text, admission, and encryption.

[gun.md](../gun.md) holds what a caller sees: `gun:pair`, `gun:user`,
`gun:secret`, `gun:encrypt` and `gun:decrypt`. This file holds the rules
behind them, and [sea.lisp](sea.lisp) holds the code. The primitives come from
the crypto plugin, whose
[README](https://github.com/elle-lisp/plugins/blob/main/crypto/README.md)
gives their arguments and failures.

## Key pairs

A user keeps one secret: a **seed** of 32 bytes. HKDF-SHA256 derives two keys
from it, with an empty salt and a fixed info:

| Key | Derivation |
|-----|------------|
| signing seed | `(crypto:hkdf-sha256 seed "" "gun/sea/sign" 32)` |
| encryption secret | `(crypto:hkdf-sha256 seed "" "gun/sea/encrypt" 32)` |

A **pair** is the struct `{:seed :pub :epub}`. `:pub` is the lowercase hex of
the Ed25519 public key of the signing seed, and `:epub` is the lowercase hex
of the X25519 public key of the encryption secret. `(gun:pair)` draws a seed
from `random:csprng-bytes`, and `(gun:pair seed)` rebuilds the pair of a
seed. A seed that is not 32 bytes raises
`{:error :gun-error :reason :bad-seed}`.

## User spaces

The soul `~<pub>` is the root of the **space** of `pub`. Every soul below it,
`~<pub>/…`, belongs to the same space. The space of a soul is its text up to
the first `/`. A soul that starts with `~` names a space, and a space whose
name after the `~` is not 64 lowercase hex characters raises
`{:error :gun-error :reason :invalid-space}`.

`(gun:user db pair)` answers the chain at `~<pub>`. That chain, and every
chain `gun:get` extends from it, carries the pair as its **signer**. `gun:put`
and `gun:set` through such a chain sign each field they write into the
signer's space. A field they write into another space stays unsigned, so
admission refuses it. `gun:set` in a space gives the new node the soul
`~<pub>/<uuid>`, so the node stays in the space. `gun:user` writes nothing.

## Signatures

A field in a space carries a third element: `[state value signature]`. The
signature is the 64-byte Ed25519 signature, under the space's key, of the
`json/serialize` text of the array `["gun/sea/sign" soul key state encoding]`,
where the encoding is the value's `value:encode` form:

```text
["gun/sea/sign","~3d40…/profile","name",117425205471346688,"s:Alice"]
```

The text names the soul and the key, so a signed field copied into another
node or another key does not verify there. It names the state, so a copied
field cannot claim a later state. A field outside every space carries no
signature. HAM compares states and values only, so the signature never
decides a merge.

On the wire a signed field is `[state, encoding, signature-hex]`, where the
hex is 128 lowercase characters. In a Redis hash it is
`"<state> <signature-hex> <encoding>"`. An encoding holds a `:` at index 1 and
the hex holds none, so the two layouts never read as each other.

## Admission

Every field passes **admission** before any merge sees it. Admission checks
each field of a soul in a space:

| Field | Reason |
|-------|--------|
| No signature | `:unsigned` |
| A signature that does not verify | `:bad-signature` |
| A signature on a soul outside every space | `:bad-signature` |

Admission runs at each place a field enters a database:

| Where | What a failure does |
|-------|---------------------|
| A local `put` or `set` | Raises the reason, and writes nothing |
| A put or an answer from a peer | Refuses the whole message before any merge. A put acks `err` with the reason. The error goes to `:on-error` |
| A read from the store | Drops the field, and gives `:bad-signature`, with the soul and the key, to `:on-error`. The rest of the node loads |

A peer forwards only the fields its merge accepted, so a forged field never
leaves the first database it reaches. Refusing the whole message keeps a peer
from merging half of a forged write.

A database opened without the crypto and random plugins cannot check a
signature. A chain that reaches a soul in a space then raises
`{:error :gun-error :reason :sea-not-configured}`, for a read and for a
write, and a peer's message that holds such a soul is refused for that
reason.

## Encryption

Encryption is explicit. `gun:encrypt` turns a value into bytes, which are a
value of their own and can be put anywhere.

- `(gun:secret pair epub)` answers a 32-byte key:
  `(crypto:hkdf-sha256 (crypto:x25519 encryption-secret epub-bytes) "" "gun/sea/secret" 32)`.
  Both sides of an X25519 exchange compute the same key, so
  `(gun:secret alice bob:epub)` equals `(gun:secret bob alice:epub)`. Pass a
  pair its own `epub` for a key that only that pair can compute. An `epub`
  that is not 64 lowercase hex characters, or whose point has small order,
  raises `{:error :gun-error :reason :bad-public-key}`.
- `(gun:encrypt key v)` answers a 24-byte random nonce followed by
  `(crypto:xchacha20poly1305-seal key nonce (value:encode v) "gun/sea/encrypt")`.
  A key that is not 32 bytes raises `:bad-key`.
- `(gun:decrypt key b)` answers the value that `gun:encrypt` sealed. Bytes
  shorter than a nonce and a tag, a seal that does not open under the key,
  or a plaintext that does not decode raise `:decrypt-failed`.

A user publishes an encryption key by putting its `epub` in its own space,
where nobody else can replace it. [gun.md](../gun.md) shows the put.
