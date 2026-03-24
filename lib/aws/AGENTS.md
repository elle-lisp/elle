# AWS Module

## Purpose

Elle-native AWS client. No Rust AWS SDK — pure Elle HTTP over TLS with
SigV4 signing. Service modules are generated from AWS Smithy models.

## Files

| File | Role |
|------|------|
| `lib/aws.lisp` | Core: HTTPS request lifecycle, chunked decoding, SigV4 integration |
| `lib/aws/sigv4.lisp` | SigV4 signing: canonical request, string-to-sign, HMAC key derivation |
| `lib/aws/*.lisp` | Generated service modules (gitignored, one per service) |

## Generating service modules

Generated `.lisp` files are gitignored. To create them:

```bash
# One step: fetch model + generate
elle tools/aws/aws-gen.lisp -- s3 dynamodb sts

# Manual: fetch then generate
elle tools/aws/fetch-model.lisp -- s3
elle tools/aws/aws-codegen.lisp -- s3 > lib/aws/s3.lisp
```

Models are cached in `aws-models/` (also gitignored). Generation is
skipped if the output is newer than the model.

## Codegen tools

| Tool | Input | Output |
|------|-------|--------|
| `tools/aws/aws-gen.lisp` | Service names | Fetches models + generates modules |
| `tools/aws/aws-codegen.lisp` | `aws-models/{svc}.json` | Elle module on stdout |
| `tools/aws/fetch-model.lisp` | Service names | `aws-models/{svc}.json` via HTTPS |

## Protocol support

The codegen reads `smithy.api#http` traits for REST services and
protocol traits for JSON-RPC/Query services:

- **restXml / restJson1**: HTTP method + URI template + query/header bindings
- **awsJson1_0 / awsJson1_1**: POST `/` with `X-Amz-Target` header + JSON body
- **awsQuery / ec2Query**: POST `/` with `Action=` form-encoded body

## Generated function signature

```lisp
(defn operation-name [required-arg1 required-arg2 &keys opts]
  ...)
```

Required params (URI labels, required query params) are positional.
Optional params (query params, headers, payload) come as keyword args.
Every module exports `:api-version`.

## Plugin dependencies

The AWS client requires three plugins at load time:

- `elle-crypto` — SHA-256 and HMAC-SHA-256 for SigV4
- `elle-jiff` — Timestamps for SigV4 date headers
- `elle-tls` — HTTPS connections via rustls

## Invariants

1. Generated files are derived artifacts — never edit them directly.
2. `lib/aws.lisp` and `lib/aws/sigv4.lisp` are hand-written and checked in.
3. The codegen is deterministic: same model → same output.
4. API version is embedded in both the file header and the module struct.
