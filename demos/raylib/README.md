# Raylib

<!-- audited: 2026-10-23 -->

A bouncing ball in an 800×450 window, drawn through `std/raylib` at 60 frames a
second.

[hello.lisp](hello.lisp) is the whole demo. A left click moves the ball to the
cursor, and Escape quits. `std/raylib` opens `libraylib.so` through the FFI, so
the demo needs raylib installed and a display.

```bash
elle demos/raylib/hello.lisp
```
