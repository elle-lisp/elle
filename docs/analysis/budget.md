# Where subject-mixing begins, and the source cap it sets (2026-10-19)

<!-- audited: 2026-10-20 -->

The share of Elle's sources that do more than one job, by size, and the
2,500-token cap read off it.

**Verdict: the share of sources naming three governing documents steps from
1.7 % to 9.0 % at 2,500 tokens. The cap is 2,500 tokens. That puts 628 of the
2,443 sources over it, against 17 at 500 lines.**

[The documentation policy](../../DOCUMENTATION.md) states the cap, and
[the size producer](../../tests/ratchet/size.lisp) holds each source to it.

## What is measured

The subjects are the sources at `583bc163f` on `main`: the files the audit
queue walks that are not documents, `.rs`, `.lisp` and `scripts/*`
([the audit queue](../impl/audit.md)). A file's size is the tokens of its whole
text in the `o200k_base` encoding. Its documents are the distinct `.md` paths
its first twelve lines name in a comment, read the way
[the prose checks](../../tests/prose.rs) read a header.

The proxy for more than one subject is the policy's own: a source that names
three governing documents. A source with no header names nothing yet, so the
share is taken over the 1,700 sources with one. Of those, 48 name three, and
none names more.

```sh
make budget
```

That prints the curve below, both bin widths and both denominators.

## A line is two budgets

A line costs a reader between seven and sixteen tokens, by kind:

| file | lines | tokens | per line |
| --- | --- | --- | --- |
| [docs/ratchet.md](../ratchet.md) | 379 | 5 427 | 14.3 |
| [lib/http.lisp](../../lib/http.lisp) | 436 | 4 292 | 9.8 |
| [src/vm/core.rs](../../src/vm/core.rs) | 426 | 5 393 | 12.7 |
| [src/value/heap.rs](../../src/value/heap.rs) | 418 | 3 580 | 8.6 |

Over the files of 100 lines or more, the spread holds by kind:

| kind | files | p5 | median | p95 |
| --- | --- | --- | --- | --- |
| Rust | 1 168 | 7.2 | 9.4 | 12.1 |
| Lisp | 462 | 8.6 | 11.0 | 14.4 |
| document | 200 | 7.4 | 12.3 | 16.3 |

So 500 lines is about 4,700 tokens of Rust, 5,500 of Lisp and 6,000 of prose.

## The curve

| tokens | sources | name three | share |
| --- | --- | --- | --- |
| 0–499 | 165 | 1 | 0.6 % |
| 500–999 | 319 | 2 | 0.6 % |
| 1 000–1 499 | 303 | 4 | 1.3 % |
| 1 500–1 999 | 241 | 2 | 0.8 % |
| 2 000–2 499 | 181 | 3 | 1.7 % |
| 2 500–2 999 | 122 | 11 | 9.0 % |
| 3 000–3 499 | 123 | 7 | 5.7 % |
| 3 500–3 999 | 97 | 8 | 8.2 % |
| 4 000–4 499 | 66 | 6 | 9.1 % |
| 4 500–4 999 | 41 | 2 | 4.9 % |
| 5 000–5 499 | 26 | 2 | 7.7 % |
| 5 500 and over | 16 | 0 | 0 % |

Under 2,500 tokens the share stays under 2 %. At 2,500 it steps to 9.0 %, and
past it no bin with more than thirty sources reads under 4.9 %. No source over
6,000 tokens names three documents.

The thousand-token bins blur the step across two bins:

| tokens | sources | name three | share |
| --- | --- | --- | --- |
| 0–999 | 484 | 3 | 0.6 % |
| 1 000–1 999 | 544 | 6 | 1.1 % |
| 2 000–2 999 | 303 | 14 | 4.6 % |
| 3 000–3 999 | 220 | 15 | 6.8 % |
| 4 000–4 999 | 107 | 8 | 7.5 % |
| 5 000 and over | 42 | 2 | 4.8 % |

Read by language, Rust alone passes 5 % in the 2,000 bin, at 5.2 %, and Lisp
never steps: 6 of its 584 headed sources name three. The two languages share
one cap, because the Lisp curve gives no case for another.

In lines the share also climbs, from 3.8 % at 200–299 lines to 8.1 % at
300–399:

| lines | sources | name three | share |
| --- | --- | --- | --- |
| 0–99 | 493 | 3 | 0.6 % |
| 100–199 | 537 | 7 | 1.3 % |
| 200–299 | 292 | 11 | 3.8 % |
| 300–399 | 223 | 18 | 8.1 % |
| 400–499 | 152 | 9 | 5.9 % |
| 500 and over | 3 | 0 | 0 % |

The cap is in tokens all the same, because a line of Lisp costs a reader a
sixth more than a line of Rust.

## The denominator

Counted over every source, with a source that has no header naming none, the
thousand-token shares are 0.4, 0.8, 3.3, 5.5 and 5.8 %, and 3.7 % from
5,000 tokens up. The step stays between the 2,000 and the 3,000 bin.

A 500-token bin near the step holds 97 to 181 sources, so one hit moves its
share by about a point. The step at 2,500 is seven points, so it stands well
above that noise.

## What the cap costs

| cap | sources over | tokens over | name three, caught |
| --- | --- | --- | --- |
| 2 000 tokens | 884 | 1 131 310 | 39 of 48 |
| 2 500 tokens | 628 | 757 913 | 35 of 48 |
| 3 000 tokens | 465 | 486 262 | 25 of 48 |
| 3 500 tokens | 309 | 292 326 | 18 of 48 |
| 500 lines | 17 | 3 551 lines | 0 of 48 |

The cap at 500 lines catches none of the 48 sources naming three documents. The
cap at 2,500 tokens catches 35.

The ratchet holds every ledgered source to its 500-token bucket until it is
split, so on the day the cap moves a quarter of the sources cannot grow past
where they stand. Summed over their buckets, the sources' excess is 923,000
tokens.

## What this does not measure

- **Documents.** A document has no one-subject proxy yet, so this report reads
  no document cap off the tree.
- **The proxy itself.** A source can mix subjects under one document, and this
  count does not see that.
- **The scripts.** Three files is no curve, so the scripts take the cap of the
  sources around them.
