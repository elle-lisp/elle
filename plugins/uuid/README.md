# elle-uuid

UUID generation and parsing for Elle, via the `uuid` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_uuid.so` (or `target/release/libelle_uuid.so`).

## Usage

```lisp
(import-file "path/to/libelle_uuid.so")

(uuid/v4)                                       ;; random UUID string
(uuid/v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "example.com")  ;; deterministic UUID
(uuid/parse "550E8400-E29B-41D4-A716-446655440000")  ;; normalize to lowercase
(uuid/nil)                                      ;; "00000000-0000-0000-0000-000000000000"
(uuid/version (uuid/v4))                        ;; 4
```

## Primitives

### `uuid/v4`

**Signature:** `(uuid/v4)`

**Returns:** string (UUID v4)

**Signal:** silent

**Description:** Generate a random UUID (version 4) using OS entropy.

**Example:**
```lisp
(uuid/v4)
;; => "550e8400-e29b-41d4-a716-446655440000"
```

### `uuid/v5`

**Signature:** `(uuid/v5 namespace name)`

**Arguments:**
- `namespace` — string (UUID in canonical form)
- `name` — string (name to hash)

**Returns:** string (UUID v5)

**Signal:** errors

**Description:** Generate a deterministic UUID (version 5) from a namespace UUID and a name using SHA-1 hashing. The same namespace and name always produce the same UUID.

**Example:**
```lisp
(uuid/v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "example.com")
;; => "cfbff0d1-9375-5685-968c-48ce8b15ae17"
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `namespace` is not a string | `type-error` | `"uuid/v5: expected string for namespace, got {type}"` |
| `name` is not a string | `type-error` | `"uuid/v5: expected string for name, got {type}"` |
| `namespace` is not a valid UUID | `uuid-error` | `"uuid/v5: invalid namespace UUID: {reason}"` |

### `uuid/parse`

**Signature:** `(uuid/parse s)`

**Arguments:**
- `s` — string (UUID in any valid format)

**Returns:** string (normalized UUID in lowercase hyphenated form)

**Signal:** errors

**Description:** Parse and normalize a UUID string. Accepts uppercase, lowercase, and mixed case. Returns the canonical lowercase hyphenated form.

**Example:**
```lisp
(uuid/parse "550E8400-E29B-41D4-A716-446655440000")
;; => "550e8400-e29b-41d4-a716-446655440000"
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `s` is not a string | `type-error` | `"uuid/parse: expected string, got {type}"` |
| `s` is not a valid UUID | `uuid-error` | `"uuid/parse: {reason}"` |

### `uuid/nil`

**Signature:** `(uuid/nil)`

**Returns:** string (all-zeros UUID)

**Signal:** silent

**Description:** Return the nil UUID (all zeros). This is the identity UUID: `00000000-0000-0000-0000-000000000000`.

**Example:**
```lisp
(uuid/nil)
;; => "00000000-0000-0000-0000-000000000000"
```

### `uuid/version`

**Signature:** `(uuid/version uuid-str)`

**Arguments:**
- `uuid-str` — string (UUID in canonical form)

**Returns:** integer (version number 1–5) or nil (for nil UUID)

**Signal:** errors

**Description:** Extract the version number from a UUID string. Returns the version as an integer (1–5), or nil if the UUID is the nil UUID (version 0).

**Example:**
```lisp
(uuid/version (uuid/v4))
;; => 4

(uuid/version (uuid/v5 "6ba7b810-9dad-11d1-80b4-00c04fd430c8" "test"))
;; => 5

(uuid/version (uuid/nil))
;; => nil
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `uuid-str` is not a string | `type-error` | `"uuid/version: expected string, got {type}"` |
| `uuid-str` is not a valid UUID | `uuid-error` | `"uuid/version: {reason}"` |

## Summary table

| Name | Args | Returns | Signal |
|------|------|---------|--------|
| `uuid/v4` | — | string (UUID) | silent |
| `uuid/v5` | namespace (string), name (string) | string (UUID) | errors |
| `uuid/parse` | s (string) | string (normalized UUID) | errors |
| `uuid/nil` | — | string | silent |
| `uuid/version` | uuid-str (string) | integer or nil | errors |
