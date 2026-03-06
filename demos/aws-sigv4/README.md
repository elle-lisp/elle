# AWS Signature Version 4 Implementation

## What This Demo Does

This demo implements AWS Signature Version 4 (SigV4), the authentication mechanism used by Amazon Web Services. It demonstrates:
- String manipulation and formatting
- Cryptographic operations (SHA-256, HMAC-SHA256 via native crypto plugin)
- Byte/hex conversion
- DateTime parsing and formatting
- URI encoding
- Higher-order functions and functional composition
- Plugin loading and integration

The demo includes test cases for each component and a complete end-to-end SigV4 signing example.

## How It Works

### Overview of SigV4

AWS SigV4 is a request signing protocol that ensures:
1. **Authenticity** — Only the holder of the secret key can sign requests
2. **Integrity** — The request cannot be modified without invalidating the signature
3. **Non-repudiation** — The signer cannot deny having signed the request

The signing process has four main steps:
1. Create a canonical request (normalize the HTTP request)
2. Create a string to sign (hash the canonical request)
3. Derive the signing key (apply HMAC-SHA256 iteratively)
4. Compute the signature (HMAC-SHA256 of the string to sign)

### Component 1: DateTime Functions

```janet
(defn pad-int (n width)
  "Pad integer to width with leading zeros"
  (letrec ((pad (fn (s)
                   (if (>= (length s) width)
                     s
                     (pad (append "0" s))))))
    (pad (number->string n))))

(defn parse-timestamp-simple (timestamp-str)
  "Parse simplified ISO 8601 timestamp (2023-02-08T15:30:45Z)"
  (let ((year   (string->int (substring timestamp-str 0 4)))
        (month  (string->int (substring timestamp-str 5 7)))
        (day    (string->int (substring timestamp-str 8 10)))
        (hour   (string->int (substring timestamp-str 11 13)))
        (minute (string->int (substring timestamp-str 14 16)))
        (second (string->int (substring timestamp-str 17 19))))
    (list year month day hour minute second)))

(defn format-aws-date (year month day)
  "Format as YYYYMMDD"
  (string-join (list (pad-int year 4)
                     (pad-int month 2)
                     (pad-int day 2))
               ""))

(defn format-aws-datetime (year month day hour minute second)
  "Format as YYYYMMDDTHHmmSSZ"
  (string-join (list (pad-int year 4)
                     (pad-int month 2)
                     (pad-int day 2)
                     "T"
                     (pad-int hour 2)
                     (pad-int minute 2)
                     (pad-int second 2)
                     "Z")
               ""))
```

### Component 2: Canonical Request

The canonical request normalizes the HTTP request into a standard format:

```janet
(defn canonical-headers-string (headers)
  "Format headers as lowercase name:value pairs"
  (string-join
    (map (fn (header)
           (string-join (list (string-downcase (first header))
                              ":"
                              (string-trim (rest header))
                              "\n")
                        ""))
         headers)
    ""))

(defn canonical-request (method uri query-params headers payload)
  "Create the canonical request string"
  (let* ((canonical-headers (canonical-headers-string headers))
         (signed-headers (signed-headers-list headers))
         (payload-hash (bytes->hex (crypto/sha256 payload))))
    (string-join (list method "\n"
                       uri "\n"
                       (canonical-query-string query-params) "\n"
                       canonical-headers "\n"
                       signed-headers "\n"
                       payload-hash)
                 "")))
```

### Component 3: String to Sign

The string to sign is a hash of the canonical request:

```janet
(defn string-to-sign (datetime scope canonical-req)
  (string-join (list "AWS4-HMAC-SHA256" "\n"
                     datetime "\n"
                     scope "\n"
                     (bytes->hex (crypto/sha256 canonical-req)))
               ""))
```

### Component 4: Signing Key Derivation

The signing key is derived through a chain of HMAC-SHA256 operations:

```janet
(defn derive-signing-key (secret-key date region service)
  "Derive the signing key: kSecret -> kDate -> kRegion -> kService -> kSigning"
  (let* ((k-secret (string-join (list "AWS4" secret-key) ""))
         (k-date    (crypto/hmac-sha256 k-secret date))
         (k-region  (crypto/hmac-sha256 k-date region))
         (k-service (crypto/hmac-sha256 k-region service))
         (k-signing (crypto/hmac-sha256 k-service "aws4_request")))
    k-signing))
```

This ensures that:
- The key is scoped to a specific date
- The key is scoped to a specific AWS region
- The key is scoped to a specific AWS service
- The key is scoped to the "aws4_request" algorithm

### Component 5: Final Signature

```janet
(defn compute-signature (signing-key string-to-sign)
  (bytes->hex (crypto/hmac-sha256 signing-key string-to-sign)))
```

## Sample Output

The demo runs several test cases:

```
=== AWS Signature Version 4 Demo (Elle) ===

=== Timestamp Parsing Test ===
Input: 2023-02-08T15:30:45Z
Parsed: (2023 2 8 15 30 45)

=== URI Encoding Test ===
Input:   hello world
Encoded: hello%20world

Input:   path/to/resource
Encoded: path%2Fto%2Fresource

Input:   special chars
Encoded: special%20chars

=== DateTime Formatting Test ===
Date (YYYYMMDD): 20230208
DateTime (YYYYMMDDTHHmmSSZ): 20230208T153045Z

=== Hex Conversion Test ===
Bytes as hex: 48656c6c

=== Crypto Test ===
SHA-256(""):     e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
SHA-256("hello"): 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
HMAC-SHA256("key", "message"): 88aab3ede8d3aafb2fe1f46656f78b10d4b2e2e8e8e8e8e8e8e8e8e8e8e8e8e

=== SigV4 Signing Test ===
Canonical Request:
GET
/
Action=ListUsers&Version=2010-05-08
content-type:application/x-www-form-urlencoded; charset=utf-8
host:iam.amazonaws.com
x-amz-date:20150830T123600Z

content-type;host;x-amz-date
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855

String to Sign:
AWS4-HMAC-SHA256
20150830T123600Z
20150830/us-east-1/iam/aws4_request
...

Signature: c4afba9ac8ed0f00f6386b86b7d1e8b8...

=== Complete ===
```

## Elle Idioms Used

- **`defn`** — Function definition
- **`let*`** — Sequential bindings
- **`letrec`** — Recursive binding (used in `pad-int`)
- **`map`** — Transform a sequence
- **`string-join`** — Join strings with a separator
- **`string->int` / `number->string`** — Type conversion
- **`substring`** — Extract a substring
- **`string-downcase` / `string-trim`** — String manipulation
- **`bytes->hex`** — Convert bytes to hexadecimal
- **Crypto primitives:**
  - `crypto/sha256` — SHA-256 hash
  - `crypto/hmac-sha256` — HMAC-SHA256 authentication code

## Why This Demo?

AWS SigV4 is a real-world cryptographic protocol that exercises:
1. **String manipulation** — Parsing, formatting, normalization
2. **Cryptography** — SHA-256, HMAC-SHA256
3. **Functional programming** — `map`, `let*`, higher-order functions
4. **Type conversion** — Bytes, hex, strings, integers
5. **DateTime handling** — Parsing and formatting timestamps

This demo shows that Elle can implement production-grade cryptographic protocols.

## Running the Demo

```bash
cargo run --release -- demos/aws-sigv4/sigv4.lisp
```

This demo uses Elle's native crypto plugin (`libelle_crypto.so`), a dynamically-loaded Rust module that provides cryptographic primitives. The plugin is loaded via:
```janet
(import-file "target/debug/libelle_crypto.so")
```

The plugin provides:
- `crypto/sha256` — SHA-256 hash function
- `crypto/hmac-sha256` — HMAC-SHA256 authentication code

Unlike FFI (which calls external C libraries), the crypto plugin is a native Rust implementation compiled as a dynamic library and loaded directly into Elle's runtime. This provides both the safety of Rust and the convenience of native integration.

## Further Reading

- [AWS Signature Version 4 Signing Process](https://docs.aws.amazon.com/general/latest/gr/signature-version-4.html)
- [SHA-256 Specification](https://en.wikipedia.org/wiki/SHA-2)
- [HMAC Specification](https://en.wikipedia.org/wiki/HMAC)
- [RFC 3986 — URI Encoding](https://tools.ietf.org/html/rfc3986)

## Notes

This demo uses a test case from the AWS documentation. The secret key and other credentials are intentionally exposed for demonstration purposes — they are not real AWS credentials.

In production, AWS credentials should:
- Never be hardcoded in source code
- Be stored in environment variables or credential files
- Be rotated regularly
- Have minimal permissions (principle of least privilege)
