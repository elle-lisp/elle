# Elle AWS Client

<!-- audited: 2026-09-23 -->

An AWS client in Elle: SigV4 signing, HTTPS over the tls plugin, and service modules generated from AWS Smithy models.

[AGENTS.md](AGENTS.md) holds the files, the generator and the invariants.
This guide holds what a caller needs.

## Loading

The core client takes three plugins by name: `(import "plugin/crypto")`
for SHA-256 and HMAC, `(import "plugin/jiff")` for the signing
timestamp, and the `std/tls` module built from `(import "plugin/tls")`:

```lisp
(defn aws-client [crypto jiff tls-plugin]
  "The core AWS client."
  ((import "std/aws") :crypto crypto :jiff jiff
                      :tls ((import "std/tls") tls-plugin)))
```

Importing `std/aws` reads the credentials from `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`. The default region is
`AWS_DEFAULT_REGION`, then `AWS_REGION`, then `us-east-1`.

## A request

The core client exports one function, `aws:request`. It takes a service
name, a method, a path, and an optional struct of `:region`, `:query`,
`:headers`, `:body` and `:raw`:

```lisp
(defn list-buckets [aws]
  "S3's ListBuckets answer for us-east-1."
  (aws:request "s3" "GET" "/" {:region "us-east-1"}))
```

Each request opens its own TLS connection and closes it. The answer is
`{:status :headers :body}`. The body is parsed JSON when the content type
names JSON, a string when it names text or XML, and bytes otherwise;
`:raw true` keeps the bytes.

## Generated service modules

Generate a module with the tools, then pass it the core client:

```bash
elle tools/aws/aws-gen.lisp -- s3 dynamodb sts
```

```lisp
(defn aws-services [aws]
  "The generated S3, DynamoDB and STS modules."
  {:s3       ((import "std/aws/s3") aws)
   :dynamodb ((import "std/aws/dynamodb") aws)
   :sts      ((import "std/aws/sts") aws)})
```

A generated function takes its required members positionally, sorted by
member name, and every other member as a keyword argument. For a REST
service, the required members are the URI labels and the required query
parameters. `:region` and the other keys `aws:request` reads pass through
as keyword arguments too. Every module exports `:api-version`:

```lisp
(defn aws-examples [services]
  "One call to each generated module."
  (let [s3 services:s3
        dynamodb services:dynamodb
        sts services:sts]
    [(s3:get-object "my-bucket" "path/to/key" :range "bytes=0-99")
     (dynamodb:get-item {"id" {"S" "123"}} "my-table")   # Key, then TableName
     (sts:assume-role "arn:aws:iam::123:role/foo" "session"
                      :duration-seconds "3600")
     s3:api-version]))
```

The REST generator (restXml and restJson1, so S3 and Lambda) writes a
`let*` binding form that epoch 12 no longer reads, so those generated
modules do not compile today.
