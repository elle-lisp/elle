# GPU compute

<!-- audited: 2026-10-23 -->

Two compute kernels that Elle writes as SPIR-V at run time and runs on Vulkan
through `std/gpu` and `plugin/vulkan`.

| Program | What it does |
|---------|--------------|
| [vecadd.lisp](vecadd.lisp) | Adds two arrays of 1024 floats in 4 workgroups of 256, and asserts four of the sums |
| [mandelbrot.lisp](mandelbrot.lisp) | Counts the escape iterations of each pixel of a 512×512 Mandelbrot set, up to 256, and asserts the result count and the center pixel |

Both need a Vulkan device, and the Vulkan plugin, which `make plugins-all`
builds ([plugins](../../docs/plugins.md)). [GPU compute](../../docs/impl/gpu.md)
owns the design.

```bash
elle demos/gpu/vecadd.lisp
elle demos/gpu/mandelbrot.lisp
```
