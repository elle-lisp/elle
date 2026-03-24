# Elle AWS Client

Elle-native AWS client with SigV4 signing, HTTPS via rustls, and
auto-generated service modules from AWS Smithy models.

## Quick start

```bash
# Generate a service module (fetches model if needed)
elle tools/aws/aws-gen.lisp -- s3

# Or fetch and generate separately
elle tools/aws/fetch-model.lisp -- s3 dynamodb sts
elle tools/aws/aws-codegen.lisp -- s3 > lib/aws/s3.lisp
```

```lisp
# Load plugins + aws client
(def crypto (import-file "target/debug/libelle_crypto.so"))
(def jiff   (import-file "target/debug/libelle_jiff.so"))
(def tls-p  (import-file "target/debug/libelle_tls.so"))
(def tls    ((import-file "lib/tls.lisp") tls-p))
(def aws    ((import-file "lib/aws.lisp") crypto jiff tls))

# Load generated service module
(def s3 ((import-file "lib/aws/s3.lisp") aws))

# Use it
(ev/run (fn []
  (println s3:api-version)                     # "2006-03-01"
  (println (s3:list-buckets))                  # {:status 200 ...}
  (println (s3:get-object "bucket" "key"))     # {:status 200 ...}
  (s3:put-object "bucket" "key" :body "data") # keyword args
  (s3:list-objects-v2 "bucket" :prefix "dir/" :max-keys "10")))
```

## Architecture

- `lib/aws.lisp` — Core HTTP client: SigV4 signing, TLS connection,
  request/response wire format, chunked transfer decoding
- `lib/aws/sigv4.lisp` — Pure Elle SigV4 signing (SHA-256 + HMAC via
  elle-crypto plugin)
- `lib/aws/*.lisp` — Generated service modules (gitignored)

## Supported protocols

The codegen handles all four AWS protocol families:

| Protocol | Services | Mechanism |
|----------|----------|-----------|
| restXml | S3, CloudFront, Route53 | HTTP method + URI template |
| restJson1 | Lambda, API Gateway | REST with JSON body |
| awsJson1_0/1_1 | DynamoDB, SQS | POST `/` + `X-Amz-Target` header |
| awsQuery | STS, IAM, SNS, EC2 | POST `/` + `Action=` form body |

## Generated module API

Each generated function takes required params as positional args and
optional params as keyword args via `&keys`:

```lisp
# Positional: bucket, key. Keywords: range, region, etc.
(s3:get-object "my-bucket" "path/to/key" :range "bytes=0-99")

# Positional: table-name. Keywords: key, etc.
(dynamodb:get-item "my-table" :key {"id" {"S" "123"}})

# Positional: role-arn, role-session-name. Keywords: duration, etc.
(sts:assume-role "arn:aws:iam::123:role/foo" "session" :duration-seconds "3600")
```

Every module exports `:api-version` with the AWS API version string.

## Tools

| File | Purpose |
|------|---------|
| `tools/aws/aws-gen.lisp` | Fetch + generate in one step |
| `tools/aws/aws-codegen.lisp` | Generate Elle module from Smithy model JSON |
| `tools/aws/fetch-model.lisp` | Download Smithy model from aws-sdk-rust repo |
