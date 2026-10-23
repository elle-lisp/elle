# egui

<!-- audited: 2026-10-23 -->

Two programs that draw an immediate-mode window through `plugin/egui`, which
`std/egui` wraps.

| Program | What it does |
|---------|--------------|
| [counter.lisp](counter.lisp) | A window with a count and two buttons that change it, redrawn each frame by `ui:run` |
| [smoke.lisp](smoke.lisp) | Opens a window, draws one frame, closes it, and asserts each step |

Both need a display, and the egui plugin, which `make plugins-all` builds
([plugins](../../docs/plugins.md)).

```bash
elle demos/egui/counter.lisp
elle demos/egui/smoke.lisp
```
