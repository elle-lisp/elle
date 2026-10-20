# The plugins submodule in CI

<!-- audited: 2026-10-19 -->

How CI compiles the `plugins/` submodule, asserts what it built, and builds the plugins the language suite loads.

## The plugins job

The `plugins/` submodule is a separate cargo workspace. Every plugin in it
takes `elle-plugin` by path, so a rebuild moves the plugin and the SDK
together: a changed `elle_api!` declaration either compiles at every call site
or does not, and a job that never checks the submodule out sees neither.

The ABI version guard does not close that gap. It compares `ABI_VERSION` when
a plugin loads, so it catches a stale `.so` built against an older SDK. A
source break never reaches a load. Only a job that compiles the submodule sees
one.

So `Plugin Tests` checks the submodule out at its recorded pointer, builds
every plugin in the workspace, asserts the portable artifacts, and runs
`plugins/tests/*.lisp` against the release binary. Building at the recorded
pointer is also what keeps the pointer fresh: a pointer left behind an ABI
change names plugins that no longer compile, and the job fails on them.

### Why the job builds every plugin, not the portable set

`make plugins` builds the packages in `plugins/Makefile`'s `PORTABLE` list.
Five plugins sit outside that list — `elle-arrow`, `elle-polars`,
`elle-vulkan`, `elle-egui` and `elle-wayland` — so a job that stopped at `make
plugins` left the hole above open for them. The break this job exists to catch
is a compile error, so compiling every plugin is the whole gate. The job runs
`make plugins-all`, which builds the workspace.

The five cost nothing extra on the runner. Each reaches its system library
through `dlopen` rather than through the linker: `ash` is taken with the
`loaded` feature, and `wayland-client`, `winit` and `glutin` open theirs at run
time. `ldd` over the five artifacts names nothing outside libc, and the five
build from a cold target directory with `PKG_CONFIG` pointed at a program that
always fails. The install list below is therefore the same list `make plugins`
needed.

`PORTABLE` still decides what the artifact assertion demands and what the
corpus can rely on. It is a statement about what is safe to **run** on a
headless runner, not about what compiles.

### What the runner has to install

Two plugins in the portable set are not pure Rust, and neither says so in its
own `Cargo.toml`. The system libraries arrive several crates deep.
`elle-oxigraph` reaches `oxrocksdb-sys`, which runs bindgen over vendored
RocksDB and needs libclang. `elle-plotters` reaches `font-kit`, which needs
fontconfig through pkg-config and pulls the freetype, expat and png headers
behind it.

A manifest scan finds none of that. Two other readings do find it, and they
answer different questions.

For what the plugins **link**, read the built artifacts. After a local `make
plugins`, `ldd target/release/libelle_*.so` names every shared library and
which plugin needs it. Today that is `libfontconfig`, `libfreetype`,
`libexpat`, `libpng16` and their compression chain under
`libelle_plotters.so`, plus `libstdc++` under `libelle_oxigraph.so`, and
nothing under any other portable plugin.

For what the plugins **build with**, `ldd` says nothing: a build-time tool
leaves no trace in the artifact. `oxrocksdb-sys` runs bindgen, which dlopens
libclang at build time and links none of it. That class shows up only in a
build on a bare runner, so read it out of the failure and set the variable the
tool asks for.
The job locates `libclang.so` rather than naming an LLVM version, because the
version moves with the runner image.

### Why the job asserts its build output

Each `plugins/tests/*.lisp` imports its `.so` under `protect` and exits 0 when
the import fails. That is deliberate, because the file has to run on a tree
where the submodule was never built. It also means a plugin that did not build
makes its own test report success.

`make plugins-verify` asserts the build output separately. Every package in
`plugins/Makefile`'s `PORTABLE` list must have produced its cdylib under
`target/release`. The list is read back out of the submodule's own make rather
than copied, so a plugin added there is demanded here with no second edit. With
the artifacts asserted, the self-gating is harmless — the tests never have to be
the thing that detects a missing plugin. `make smoke-plugins` runs the
assertion before the corpus for the same reason.

The assertion names the portable set, not every plugin, because the two lists
answer different questions. Compiling is what the job builds all of them for.
Asserting an artifact matters where a corpus file would otherwise report
success over a plugin that never built. `elle-vulkan`, `elle-egui` and
`elle-wayland` need a GPU or a display before they can do anything a corpus
file could assert.

That leaves one thing for a test to hold. `make plugins-all` compiles the
members of `plugins/Cargo.toml`'s workspace, so a plugin directory missing from
that list is compiled by nothing and asserted by nothing, and the job stays
green over it. [plugins.rs](../../tests/integration/plugins.rs) pins every
plugin directory to a workspace member.

## The plugins the language suite loads

A language-suite file that needs a plugin imports it under `protect`, and gates
itself when the import fails. A build without the plugin therefore skips the
file, and the run reports nothing wrong. The loopback tests in
[websocket.lisp](../../tests/lang/websocket.lisp) and the `std/gun` mesh files
need `plugin/hash` and `plugin/random`, and the `std/gun` user-space files need
`plugin/crypto` too. All three live in the `plugins/` submodule. Until the
suite jobs built them, no suite job ran those tests.

So every job that runs the language suite checks the submodule out and runs
`make suite-plugins` before its first suite or doctest step. The target builds
each package that `SUITE_PLUGINS` names into the build's own `target`
directory, where `plugin/X` resolves. Then `suite-plugins-verify` asserts each
artifact, for the reason `plugins-verify` asserts its own: a plugin that did
not build would only make its files skip. A document gates a plugin example the
same way, so the doctest steps come after the build too.

`SUITE_PLUGINS` names what the language suite imports and nothing more, because
each name costs every suite job a build. The implementation suite's GPU files
import `plugin/vulkan`, which needs a GPU, so no job builds it for them.
[plugins.rs](../../tests/integration/plugins.rs) is the standing check on all
three claims: every plugin a language-suite file imports is in `SUITE_PLUGINS`,
the assertion fails on a missing artifact, and every job that runs the language
suite, in every workflow, builds them first.
