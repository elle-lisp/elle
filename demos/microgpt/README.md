# microgpt

<!-- audited: 2026-10-23 -->

A port of Karpathy's microgpt: a scalar autograd engine and a one-layer
character-level transformer, trained on names and then sampled.

| File | What it holds |
|------|---------------|
| [microgpt.lisp](microgpt.lisp) | The tokenizer, Adam, the training loop and sampling; the program to run |
| [autograd.lisp](autograd.lisp) | The scalar autograd engine: each value a mutable struct with its children and local gradients |
| [model.lisp](model.lisp) | The model: 16-wide embeddings, 4 heads, one layer, a 16-token block, RMSNorm and a KV cache |
| [helpers.lisp](helpers.lisp) | A 2-D array builder and the KV caches |
| [microgpt.py](microgpt.py) | The Python original, for comparison |
| `input.txt` | The 484 names both programs train on |

The program checks two gradients, then trains for 1000 steps with the random
generator seeded at 42, and prints 20 names it samples. It needs
`plugin/random`, which `make plugins` builds
([plugins](../../docs/plugins.md)). Run it from the repository root, because it
imports its modules and reads `input.txt` by paths from there.

```bash
elle demos/microgpt/microgpt.lisp
```
