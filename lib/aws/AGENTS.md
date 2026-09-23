# AWS Module

<!-- audited: 2026-09-23 -->

The implementor's guide to the AWS client: its files, the generator that
writes the service modules, and the invariants they keep. A caller reads
[README.md](README.md) instead.

## Files

| File | Role |
|------|------|
| [aws.lisp](../aws.lisp) | Core: one HTTPS request per call, the response reader, chunked decoding, SigV4 through `sigv4.lisp` |
| [sigv4.lisp](sigv4.lisp) | SigV4 signing: canonical request, string-to-sign, HMAC key derivation |
| `lib/aws/<service>.lisp` | Generated service modules, one per service, ignored by git |

## The generator

Three tools under `tools/aws/` write the service modules:

| Tool | Input | Output |
|------|-------|--------|
| [aws-gen.lisp](../../tools/aws/aws-gen.lisp) | Service names | Fetches each missing model, then generates each module that is missing or older than its model |
| [aws-codegen.lisp](../../tools/aws/aws-codegen.lisp) | `aws-models/<service>.json` | The Elle module, on stdout |
| [fetch-model.lisp](../../tools/aws/fetch-model.lisp) | Service names | `aws-models/<service>.json`, over HTTPS |

```bash
# One step: fetch the models and generate the modules
elle tools/aws/aws-gen.lisp -- s3 dynamodb sts

# By hand: fetch, then generate
elle tools/aws/fetch-model.lisp -- s3
elle tools/aws/aws-codegen.lisp -- s3 > lib/aws/s3.lisp
```

The models live in `aws-models/`, which git ignores. The fetch tools load
the tls plugin from `target/debug/`, and `aws-gen.lisp` runs the
generator with `ELLE_BIN`, by default `./target/debug/elle`. Both need a
debug build.

## What the generator emits

The generator reads the protocol off the model's service shape:

- **restXml and restJson1**: the HTTP method and the URI template of each
  operation, with its query and header bindings.
- **awsJson1_0 and awsJson1_1**: `POST /` with an `X-Amz-Target` header and
  a JSON body.
- **awsQuery and ec2Query**: `POST /` with an `Action=` form body.

A module is a function of the core client, `(fn [aws] ...)`, and each
operation becomes one function,
`(defn operation-name [required-arg1 required-arg2 &keys opts] ...)`, that
calls `aws:request` with the service name. The required members are
positional, in the order their names sort,
because the generator walks the model's members through `pairs`. Every
other member is a keyword argument. Every module exports `:api-version`,
and its header comment names the same version.

The REST emitter writes `(let* [[opts (or opts {})] [path ...]] ...)`, a
binding form that epoch 12 no longer reads. A restXml or restJson1 module
therefore does not compile today; the awsJson and awsQuery emitters bind
with `def` and do.

## Invariants

1. A generated file is a derived artifact. Regenerate it; never edit it.
2. [aws.lisp](../aws.lisp) and [sigv4.lisp](sigv4.lisp) are hand-written
   and checked in.
3. The generator is deterministic: the same model gives the same output.
