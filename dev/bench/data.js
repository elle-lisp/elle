window.BENCHMARK_DATA = {
  "lastUpdate": 1772863012389,
  "repoUrl": "https://github.com/elle-lisp/elle",
  "entries": {
    "Elle Benchmarks": [
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": false,
          "id": "73f55acfa3727b48a2376170448d2603e70ba437",
          "message": "Add (environment) primitive and defined-globals tracking (#485)\n\n* Add defined_globals tracking to VM for O(defined) environment enumeration\n\n* Add (environment) primitive via SIG_QUERY for runtime introspection\n\n* Fix rustdoc warning: escape angle brackets in environment doc comment",
          "timestamp": "2026-03-05T20:26:22Z",
          "tree_id": "e66304d18d1707d54088783daecf5aeccb670740",
          "url": "https://github.com/elle-lisp/elle/commit/73f55acfa3727b48a2376170448d2603e70ba437"
        },
        "date": 1772746310500,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 158,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1306,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2193,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1401,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 25369,
            "range": "± 534",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 75,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 18638,
            "range": "± 126",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 355006,
            "range": "± 25423",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 460471,
            "range": "± 31776",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 465992,
            "range": "± 8240",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 567,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 441,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 280,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1002,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 877,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 552,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 7333,
            "range": "± 490",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 620729,
            "range": "± 26411",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 637914,
            "range": "± 20398",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1776,
            "range": "± 49",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 810,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 32457,
            "range": "± 3937",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 22512,
            "range": "± 1272",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 62870,
            "range": "± 9336",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 43694,
            "range": "± 2494",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 281270,
            "range": "± 43208",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 195754,
            "range": "± 21501",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 113,
            "range": "± 78",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "41766f4d5f4397af35b67c523d98a35700543f66",
          "message": "escape analysis (#421)\n\n* migrate prelude and eval integration tests to Elle scripts\n\nMove 48 prelude tests and 46 eval tests from Rust integration tests\nto Elle test scripts in tests/elle/. Both files were 100% eligible\nfor migration. Delete the Rust originals and update mod.rs.\n\n* escape analysis: add 9 missing primitive aliases to whitelist, wrong-arity test\n\nAdd eq?, string-contains?, string-starts-with?, string-ends-with?,\ncoroutine?, fn/mutates-params?, mutates-params?, fn/raises?, raises?\nto IMMEDIATE_PRIMITIVES (now 48). Aliases get their own SymbolId so\nthey're invisible to both intrinsics map and whitelist unless explicit.\nAdd wrong-arity safety test.\n\n* fix: fn/raises? → fn/errors? — Elle has (error), not raise\n\nThe primitive is registered as fn/errors? with no aliases. fn/raises? and\nraises? were phantom whitelist entries silently ignored by the SymbolId\nlookup. Fixed in IMMEDIATE_PRIMITIVES and AGENTS.md.\n\n* fix: portable temp paths in eval.lisp, add list splice tests\n\n* tier proptest cases: 8 for PRs, 64 for merge queue, 128 for weekly\n\n* fix: resolve PatternKey API mismatch after rebase\n\n* migrate: move property tests to elle scripts for faster CI\n\n* ci: reduce proptest cases to 16 for regular CI (nightly/weekly run 64/128)\n\n* docs: improve doc comment for can_scope_allocate_let\n\n* ci: retry after checking for stuck runs",
          "timestamp": "2026-03-05T21:15:59Z",
          "tree_id": "e519e41a7dad5a5d61b84e7ff20c662197a9a2db",
          "url": "https://github.com/elle-lisp/elle/commit/41766f4d5f4397af35b67c523d98a35700543f66"
        },
        "date": 1772748619094,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 158,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1287,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2190,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1343,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 25030,
            "range": "± 67",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 75,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 17360,
            "range": "± 108",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 261847,
            "range": "± 24099",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 344229,
            "range": "± 32445",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 429515,
            "range": "± 60042",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 581,
            "range": "± 9",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 442,
            "range": "± 15",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 283,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1005,
            "range": "± 25",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 882,
            "range": "± 20",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 581,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 3393,
            "range": "± 657",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 557276,
            "range": "± 36115",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 573114,
            "range": "± 34659",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1774,
            "range": "± 89",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 824,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 29662,
            "range": "± 5889",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 15142,
            "range": "± 2992",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 60765,
            "range": "± 9438",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 28304,
            "range": "± 5829",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 263626,
            "range": "± 41723",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 164867,
            "range": "± 14510",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 113,
            "range": "± 1",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "e3707d6f74498590c3a923925853918db74873aa",
          "message": "escape: tier 6 — while/block/break-aware scope allocation (#488)\n\n* escape: tier 6 — while/block/break-aware scope allocation\n\nAdd While, Block, and Break handling to result_is_safe in escape\nanalysis. While always returns nil (safe). Block checks both normal\nexit (last expression) and all break values targeting the block.\nBreak is safe in result position (it jumps away, never produces a\nvalue locally).\n\nRelax can_scope_allocate_block: instead of rejecting all blocks with\nbreaks, check that all break values targeting the block are safe\nimmediates. Relax can_scope_allocate_let similarly: instead of\nrejecting all lets with breaks, check that all break values are safe.\n\nRemove dead walkers (body_contains_break, hir_contains_break,\nwalk_for_break) replaced by value-aware variants.\n\nAdd 18 new escape analysis tests (positive, negative, correctness).\nFlip 2 existing tests that now correctly emit regions.\n\nRefs #421\n\n* fix: disable scope allocation for let bindings with escaping breaks\n\n* fix: reconcile tier 6 escape analysis with merged #421 changes\n\n* fix: record region depth before RegionEnter in block lowering\n\nBreaks targeting a scope-allocated block must emit compensating\nRegionExit instructions for the block's own region. The block\ncontext's region_depth_at_entry was recorded AFTER emit_region_enter(),\nso breaks computed 0 compensating exits and leaked the region mark.\n\nFix: record depth before RegionEnter. Update the arena test to\nexpect 2 RegionExit (1 compensating from break + 1 normal exit)\nnow that the block qualifies for scope allocation under tier 6.",
          "timestamp": "2026-03-05T23:27:36Z",
          "tree_id": "750981aa2dc0cc6db9e7f5dbc9e916571e1ec6da",
          "url": "https://github.com/elle-lisp/elle/commit/e3707d6f74498590c3a923925853918db74873aa"
        },
        "date": 1772756623153,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 138,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1228,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2105,
            "range": "± 173",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1278,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 23339,
            "range": "± 34",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 61,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 7,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 18239,
            "range": "± 77",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 240812,
            "range": "± 21947",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 293907,
            "range": "± 14470",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 360372,
            "range": "± 28886",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 698,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 481,
            "range": "± 52",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 243,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1069,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 972,
            "range": "± 18",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 717,
            "range": "± 42",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 4213,
            "range": "± 997",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 467904,
            "range": "± 28813",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 516326,
            "range": "± 24152",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1673,
            "range": "± 1115",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 892,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 29064,
            "range": "± 3852",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 15931,
            "range": "± 3503",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 54674,
            "range": "± 7015",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 40082,
            "range": "± 5701",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 218421,
            "range": "± 25432",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 146967,
            "range": "± 15908",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 121,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": false,
          "id": "a1954cc37072a3bc650f2ccd8c75693458b228d7",
          "message": "Implement Racket-style parameters for dynamic bindings (#466) (#482)\n\n* Implement Racket-style parameters: value type, primitives, and callable dispatch\n\nThis commit adds the foundation for dynamic (fiber-scoped) bindings:\n\n- New heap type `Parameter` with unique id and default value\n- Primitives: `make-parameter`, `parameter?`\n- Parameters are callable: `(param)` reads current value from fiber's param_frames\n- Fiber.env field removed (unused); param_frames field added\n- 11 integration tests covering basic parameter operations\n\nThe `parameterize` special form and fiber inheritance are in subsequent commits.\n\n* Implement parameterize special form: HIR, bytecode, lowering, and VM dispatch\n\nThis commit adds the `parameterize` special form for dynamic parameter binding:\n\n- New HirKind::Parameterize variant with bindings and body\n- PushParamFrame and PopParamFrame bytecode instructions\n- Lowering from HIR to LIR with proper stack protocol\n- VM dispatch for frame push/pop with parameter validation\n- 18 integration tests covering parameterize behavior:\n  - Basic override and revert\n  - Nested parameterize with shadowing\n  - Multiple bindings\n  - Body as begin (multiple expressions)\n  - Error on non-parameter values\n\nBody is NOT in tail position (PopParamFrame must execute after).\nStack protocol: push pairs as [param1, val1, param2, val2, ...], pop in reverse.\n\n* Add child fiber parameter inheritance (phase 8, #466)\n\nOn first resume, flatten the parent's param_frames into a single frame\nand set it on the child fiber so it inherits the parent's current\ndynamic parameter bindings.\n\n* Add parameters example and documentation\n\nCreated comprehensive example demonstrating:\n- Parameter creation and reading\n- parameterize special form with override and revert\n- Nested parameterize with shadowing\n- Multiple parameters in one parameterize\n- I/O port simulation use case\n- Fiber inheritance of parameter bindings\n\nUpdated all AGENTS.md files to document:\n- Parameter heap type and runtime representation\n- parameterize special form semantics\n- Bytecode instructions (PushParamFrame, PopParamFrame)\n- HIR and LIR representations\n- VM parameter resolution and frame management\n- Primitives (make-parameter, parameter?)\n\nThe example is executable and demonstrates all major features of the parameters system.\n\n* Move parameter behavioral tests to Elle scripts\n\nMove 15 behavioral parameter tests from Rust integration tests to Elle scripts\nin tests/elle/parameters.lisp. Keep only 4 Rust tests that require type inspection\n(is_parameter, as_keyword_name, error message validation).\n\nThis follows the testing convention: Elle scripts for behavioral tests,\nRust tests for type inspection and error handling.\n\nAll 19 tests pass (4 Rust + 15 Elle).\n\n* Add missing HirKind::Parameterize arms to escape analysis\n\nThe escape analysis module has three match statements that must be exhaustive over HirKind. The Parameterize variant was added in a prior commit but the escape.rs match arms were missed, causing compilation failure in CI.",
          "timestamp": "2026-03-06T01:34:46Z",
          "tree_id": "38f7aa66bbacf44c144a76afa981522de9d95461",
          "url": "https://github.com/elle-lisp/elle/commit/a1954cc37072a3bc650f2ccd8c75693458b228d7"
        },
        "date": 1772764231184,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 160,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1313,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2228,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1339,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 26100,
            "range": "± 37",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 17152,
            "range": "± 440",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 248926,
            "range": "± 22281",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 293074,
            "range": "± 22745",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 316078,
            "range": "± 21479",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 584,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 434,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 284,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1030,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 882,
            "range": "± 16",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 593,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 2841,
            "range": "± 222",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 426012,
            "range": "± 42631",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 517090,
            "range": "± 30652",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1774,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 809,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 26111,
            "range": "± 4015",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 11157,
            "range": "± 1388",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 50495,
            "range": "± 7261",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 22199,
            "range": "± 6945",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 182402,
            "range": "± 31657",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 132708,
            "range": "± 19168",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 112,
            "range": "± 3",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a67e09f28a53c056fce5e1c120466f061a6551bd",
          "message": "Implement I/O Phase 2: Ports (#474) (#489)\n\n* Add Port type and display integration (#474)\n\nIntroduce src/port.rs with Port struct (OwnedFd-based), PortKind,\nDirection, and Encoding enums. Ports are ExternalObjects wrapping\nfile descriptors with kind-aware Display formatting. Update\nvalue display to recognize port external objects. Add integration\ntest stubs for port display.\n\n* Add port primitives (#474)\n\nImplement port/open, port/open-bytes, port/close, port/stdin,\nport/stdout, port/stderr, port?, and port/open? primitives in\nsrc/primitives/ports.rs. Register in ALL_TABLES. Add integration\ntests covering lifecycle, error handling, and display formatting.\n\n* Add standard port parameters and Elle tests (#474)\n\nDefine *stdin*, *stdout*, *stderr* as parameters in stdlib.lisp\nusing make-parameter. Add comprehensive Elle behavioral tests\ncovering port predicates, lifecycle, error handling, resource\nmanagement with `with`, and parameterize integration.\n\n* Move port tests to Elle, add mode coverage, fix basics example (#474)\n\nMigrate 14 integration tests to tests/elle/ports.lisp, keeping only\n3 Rust tests that need format inspection. Add :read-write and :append\nmode tests. Remove bare (/ 1 0) from examples/basics.lisp that caused\nCI failure.",
          "timestamp": "2026-03-06T05:03:15Z",
          "tree_id": "f618f45639d2d83eaf25a715527f9ed1092c4475",
          "url": "https://github.com/elle-lisp/elle/commit/a67e09f28a53c056fce5e1c120466f061a6551bd"
        },
        "date": 1772776755966,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 157,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1280,
            "range": "± 24",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2201,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1338,
            "range": "± 32",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 25535,
            "range": "± 126",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 76,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 18316,
            "range": "± 1284",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 270827,
            "range": "± 25655",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 409043,
            "range": "± 64878",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 443228,
            "range": "± 73386",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 570,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 444,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 280,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 999,
            "range": "± 22",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 862,
            "range": "± 27",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 554,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 4989,
            "range": "± 2115",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 614066,
            "range": "± 31584",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 633682,
            "range": "± 47662",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1949,
            "range": "± 555",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 814,
            "range": "± 12",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 29666,
            "range": "± 5361",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 27245,
            "range": "± 3759",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 63867,
            "range": "± 9333",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 49285,
            "range": "± 13635",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 297815,
            "range": "± 34219",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 263762,
            "range": "± 38513",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 109,
            "range": "± 1",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "30e9fe6a363b2f487ea791de1ad533e968e710e1",
          "message": "jit: support suspending closures via side-exit (#465)\n\n* jit: add YieldPointInfo metadata collection to emitter\n\nExtend emit() to return yield point and call site metadata alongside\nbytecode. This metadata will be used by the JIT to generate side-exit\ncode at yield points. Part of #461.\n\n* jit: relax may_suspend gate and add yield translate stubs\n\nRelax the JIT compilation gate from may_suspend() to propagates != 0,\nallowing yielding (but not polymorphic) functions to be JIT-compiled.\nAdd dead-code stubs for LoadResumeValue and trap stubs for Yield\nterminator in translate.rs. Update tests to match new acceptance\ncriteria. Part of #461.\n\n* jit: add yield runtime helpers and signal handling\n\nImplement elle_jit_yield (direct yield side-exit), elle_jit_yield_through_call\n(yield propagation through call chains), and elle_jit_has_signal (post-call\nyield detection). Replace execute_closure_bytecode with\nexecute_bytecode_saving_stack in JIT dispatch paths to support yield\npropagation. Add SIG_YIELD handling to jit_handle_primitive_signal.\nPart of #461.\n\n* jit: translate Yield terminator as side-exit\n\nReplace the Yield trap stub with real side-exit code generation: spill\nlive registers to a stack slot, call elle_jit_yield runtime helper,\nreturn YIELD_SENTINEL. LoadResumeValue remains a dead-code stub since\nresume goes through the interpreter. Part of #461.\n\n* jit: implement yield-through-call and VM integration\n\nAdd post-call yield checks in JIT-compiled call sites that detect when\na callee yields, spill live registers, and propagate YIELD_SENTINEL.\nAdd yield handling in call_inner for the JIT path to build interpreter-\nlevel caller frames. Add 6 integration tests covering yield propagation,\nresume values, multiple yields, stack preservation, nested calls, and\ncaptures. Closes #461.\n\n* fix(jit): preserve local variables across yield/resume\n\nLocal variables (parameters and let-bindings) were lost when yielding from\nJIT-compiled code and resuming. The JIT now spills locals into the\nSuspendedFrame stack in the same layout the interpreter expects, ensuring\ncorrect restoration on resume.\n\nChanges:\n- src/lir/types.rs: Add num_locals to YieldPointInfo and CallSiteInfo\n- src/lir/emit.rs: Track and record num_locals in yield/call metadata\n- src/jit/dispatch.rs: Add num_locals to YieldPointMeta; update\n  elle_jit_yield to spill locals+operands\n- src/jit/translate.rs: New spill_locals_and_operands helper; update\n  Yield terminator and post-call yield check\n- src/jit/compiler.rs: Pass num_locals to YieldPointMeta construction\n- tests/integration/jit_yield.rs: Add 2 regression tests\n\nFixes #461\n\n* jit: fix pre-merge issues from architectural review\n\n1. Add num_locals parameter to elle_jit_yield_through_call so the helper\n   can distinguish locals from operands in the spilled buffer. Document\n   as tech debt to be unified with call site metadata lookup.\n\n2. Add explicit guard in compile_batch to reject yielding functions\n   (effect.may_suspend()). Yield metadata is not propagated to shared\n   JitCode, so batch-compiled yielding functions would panic. Add\n   JitError::Yielding variant for graceful fallback.\n\n3. Fix build_closure_env_for_jit to handle variadic arity (AtLeast).\n   Replicate rest-arg collection logic from VM::populate_env. Fixes\n   potential environment corruption if JIT falls back to interpreter\n   for variadic functions.\n\n* jit: complete follow-up improvements for yield support\n\n1. Normalize signal helper parameter types: both elle_jit_has_exception\n   and elle_jit_has_signal now use u64 for vm parameter.\n\n2. Store call site metadata on JitCode: add CallSiteMeta struct and\n   call_sites field. Convert CallSiteInfo to CallSiteMeta during\n   compilation. Update elle_jit_yield_through_call to take call_site_index\n   and look up metadata from JitCode, matching elle_jit_yield pattern.\n\n3. Single shared spill slot per function: allocate one stack slot sized\n   to max spill requirement across all yield/call sites. Reuse at every\n   spill point instead of allocating per-point. Reduces frame size.\n\n4. Add layout invariant unit tests: verify JIT spill output matches\n   interpreter frame layout (params, locals, operands order). Tests\n   cover edge cases and value type preservation.\n\n* test: migrate behavioral property tests from proptest to Elle scripts\n\nMove 42 behavioral property tests that were using proptest for simple\nvalue checking into Elle test scripts. These tests don't need random\ninput generation — they're checking behavior, not algebraic invariants.\n\nMigrated:\n- matching.rs (4 tests) → tests/elle/matching.lisp\n- strings.rs (16 tests) → tests/elle/strings.lisp (appended)\n- fibers.rs (14 tests) → tests/elle/fibers.lisp\n- coroutines.rs (7 proptest blocks) → tests/elle/coroutines.lisp\n\nKept in Rust (true property tests): arithmetic, comparison, effects,\npath, reader, nanboxing, ffi (120 tests total).\n\nNet: −1,137 lines Rust proptest, +490 lines Elle test scripts.\n\n* fix: handle SIG_RESUME/SIG_PROPAGATE/SIG_CANCEL in JIT primitive signal dispatch\n\nThe JIT's jit_handle_primitive_signal panicked on signal 8 (SIG_RESUME)\nbecause it only handled SIG_OK/SIG_ERROR/SIG_HALT/SIG_YIELD/SIG_QUERY.\nWith the relaxed JIT gate allowing yielding closures, primitives like\nfiber/resume and coro/resume can now be called from JIT context and\nreturn VM-internal signals.\n\nAdd JIT-context fiber signal handlers (handle_fiber_resume_signal_jit,\nhandle_fiber_propagate_signal_jit, handle_fiber_cancel_signal_jit) to\nvm/fiber.rs that mirror the interpreter handlers but return u64 instead\nof pushing to fiber.stack. Wire them into jit_handle_primitive_signal.\n\nFixes #461\n\n* fix: use seal_all_blocks to handle LIR back-edges in JIT\n\nLIR can contain back-edges (e.g. while loops jump back to the condition\nblock). Eagerly sealing each LIR block on entry caused Cranelift to panic\nwhen a later block jumped to an already-sealed target. Replace per-block\nsealing with seal_all_blocks() after all blocks are translated.\n\n* perf: skip JIT profiling for primitives, bump CI threshold to 1.5\n\nPrimitives (closures without lir_function) can't be JIT-compiled, so\nprofiling them is pure overhead. Add guard to skip try_jit_call for\nprimitives. This eliminates the 20-30% regression on int_add and\nsymbol_interning benchmarks.\n\nAlso bump CI benchmark alert threshold from 1.2x to 1.5x to reduce\nnoise from minor fluctuations.",
          "timestamp": "2026-03-06T06:50:10Z",
          "tree_id": "afc8c4e7798046409554da0da3f750d6ee034b04",
          "url": "https://github.com/elle-lisp/elle/commit/30e9fe6a363b2f487ea791de1ad533e968e710e1"
        },
        "date": 1772783106055,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 143,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1226,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2099,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1271,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 23014,
            "range": "± 289",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 61,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 8,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 20000,
            "range": "± 92",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 284252,
            "range": "± 20831",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 368013,
            "range": "± 21814",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 421854,
            "range": "± 26956",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 708,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 444,
            "range": "± 54",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 278,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1060,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 943,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 702,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 5577,
            "range": "± 591",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 534007,
            "range": "± 17283",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 554716,
            "range": "± 27213",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1669,
            "range": "± 740",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 890,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 28256,
            "range": "± 2150",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 20207,
            "range": "± 2625",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 54243,
            "range": "± 5591",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 28520,
            "range": "± 5722",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 218134,
            "range": "± 26145",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 123144,
            "range": "± 21074",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 123,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a5a897af2a4b543a8fa98748baf1860fd6baa64c",
          "message": "Add chan/* primitives (crossbeam-channel wrapper for inter-fiber messaging) (#491)\n\n* Add chan/* primitives wrapping crossbeam-channel for inter-fiber messaging\n\n* docs: update AGENTS.md to document chan/* primitives\n\n* Migrate chan integration tests from Rust to Elle\n\nMove chan tests from tests/integration/chan.rs to tests/elle/chan.lisp,\nupdating test module registrations accordingly. This improves test\nmaintainability by keeping channel semantics tests in Elle itself.",
          "timestamp": "2026-03-06T14:29:46Z",
          "tree_id": "2c6b94669d469aeae8d7205bb6ce3a9eb31c5cab",
          "url": "https://github.com/elle-lisp/elle/commit/a5a897af2a4b543a8fa98748baf1860fd6baa64c"
        },
        "date": 1772810831069,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 159,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1295,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2217,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1349,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 25112,
            "range": "± 85",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 75,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 17875,
            "range": "± 298",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 256118,
            "range": "± 35989",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 307670,
            "range": "± 28094",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 334113,
            "range": "± 10831",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 589,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 457,
            "range": "± 23",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 288,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1003,
            "range": "± 36",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 870,
            "range": "± 26",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 562,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 2655,
            "range": "± 733",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 464094,
            "range": "± 26146",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 508514,
            "range": "± 36243",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1763,
            "range": "± 79",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 799,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 22957,
            "range": "± 4808",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 9577,
            "range": "± 639",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 48277,
            "range": "± 8762",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 20196,
            "range": "± 2697",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 185867,
            "range": "± 28155",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 115441,
            "range": "± 16806",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 102,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": false,
          "id": "8c6de8dcfbfb387e089dba8948fdc0ca301aef86",
          "message": "feat(io): I/O Phase 3 — synchronous I/O, scheduler foundation, SIG_IO (#505)\n\n* feat(io): add SIG_IO signal constant and Effect::io() / Effect::io_errors()\n\n- SIG_IO = 1 << 9 (reserved signal range, distinct from SIG_YIELD)\n- Effect::io() and Effect::io_errors() constructors\n- may_io() predicate; may_suspend() now includes SIG_IO\n- SIG_IO arm in execute_bytecode returns error instead of panicking\n\n* feat(io): add Port::with_fd() for backend fd access\n\nAllows the SyncBackend to borrow the underlying raw fd from a Port\nwithout duplicating it. Returns None for closed or stdio ports.\n\n* feat(io): add IoRequest type and src/io/ module\n\n- IoOp enum: ReadLine, Read, ReadAll, Write, Flush\n- IoRequest wraps IoOp + port as ExternalObject(\"io-request\")\n- io-request? and io-backend? type predicates\n- Integration tests for predicates\n\n* feat(io): add stream primitives (stream/read-line, read, read-all, write, flush)\n\nEach primitive validates its port argument, builds an IoRequest, and\nreturns (SIG_IO, request) to suspend the fiber for scheduler dispatch.\nEffect: io_errors() — may suspend on SIG_IO, may signal error.\n\n* feat(io): add SyncBackend with per-fd buffering, io/backend and io/execute\n\nSyncBackend executes IoRequests synchronously using libc::read/write.\nPer-fd FdState buffers preserve data across partial reads (buffer drain\ninvariant). Direction validation rejects reads from write-only ports.\nio/backend creates a backend; io/execute dispatches a single request.\n\n* feat(io): add sync-scheduler, *scheduler* parameter, and ev/spawn\n\nsync-scheduler is a trampoline that resumes fibers, dispatching SIG_IO\nrequests to a SyncBackend via io/execute. Error detection uses\nfiber/bits (not fiber/status) because caught signals always yield\n:suspended status regardless of signal type.\n\n*scheduler* is a dynamic parameter holding the active scheduler.\nev/spawn creates a fiber with SIG_ERROR|SIG_IO mask and runs it\nthrough the current *scheduler*.\n\n* feat(io): add VM::execute_scheduled() root fiber bootstrap\n\nTop-level execution now runs through *scheduler* when available.\nexecute_scheduled wraps user bytecode in a fiber (SIG_ERROR|SIG_IO\nmask), resolves *scheduler*, and calls it with the fiber. Falls back\nto direct vm.execute() when stdlib hasn't loaded yet. Updates all\nthree main.rs execution call sites.\n\n* docs(io): document I/O system — SIG_IO, IoRequest, SyncBackend, effects\n\n- AGENTS.md: SIG_IO in signal bits table, src/io/ in directories/modules\n- src/io/AGENTS.md: new file documenting IoOp, IoRequest, SyncBackend,\n  buffer drain invariant, and all io/* primitives\n- src/effects/AGENTS.md: Effect::io(), Effect::io_errors(), may_io()\n- docs/effects.md: I/O effects section explaining SIG_IO and scheduler",
          "timestamp": "2026-03-07T03:27:52Z",
          "tree_id": "304aaa09f6b2190b5cc8c69de452ad8c3a338215",
          "url": "https://github.com/elle-lisp/elle/commit/8c6de8dcfbfb387e089dba8948fdc0ca301aef86"
        },
        "date": 1772857108173,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 157,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1286,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2215,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1343,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 25243,
            "range": "± 53",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 75,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 18674,
            "range": "± 165",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 266287,
            "range": "± 35160",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 301283,
            "range": "± 31876",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 380266,
            "range": "± 38044",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 595,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 431,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 289,
            "range": "± 5",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1034,
            "range": "± 30",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 879,
            "range": "± 21",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 569,
            "range": "± 7",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 3928,
            "range": "± 1523",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 549487,
            "range": "± 51446",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 605937,
            "range": "± 42876",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1785,
            "range": "± 56",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 838,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 28697,
            "range": "± 6194",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 13792,
            "range": "± 2567",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 58473,
            "range": "± 12522",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 32315,
            "range": "± 10604",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 187698,
            "range": "± 70407",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 158023,
            "range": "± 48072",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 111,
            "range": "± 4",
            "unit": "ns/iter"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "disruptek@users.noreply.github.com",
            "name": "Smooth Operator",
            "username": "disruptek"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "a09a4af8ee14199084c110c49a3c87dbc217e7ab",
          "message": "fix: panic if effect fixpoint does not converge after 10 iterations (#506)\n\n* fix: replace 'raises'/'raise' terminology with 'signals'/'signal'\n\nElle does not unwind the stack when an error occurs — it uses a\nfiber-based signal model. The word 'raises' imported misleading\nassociations from stack-unwinding languages. All doc comments,\ndocstrings, test names, and prose now use 'signals'/'signal' instead.\n\nMathematical uses (\"raised to the power\") are unchanged.\n\n* fix: panic if effect fixpoint does not converge after 10 iterations",
          "timestamp": "2026-03-07T05:07:19Z",
          "tree_id": "d2e2970beb255f9cb7968e7dc63a2da62502465a",
          "url": "https://github.com/elle-lisp/elle/commit/a09a4af8ee14199084c110c49a3c87dbc217e7ab"
        },
        "date": 1772863011887,
        "tool": "cargo",
        "benches": [
          {
            "name": "parsing/simple_number",
            "value": 158,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/list_literal",
            "value": 1287,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/nested_expr",
            "value": 2200,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/deep_nesting",
            "value": 1338,
            "range": "± 13",
            "unit": "ns/iter"
          },
          {
            "name": "parsing/large_list_100",
            "value": 24684,
            "range": "± 45",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/first_intern",
            "value": 74,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/repeat_intern",
            "value": 9,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "symbol_interning/many_unique",
            "value": 17879,
            "range": "± 111",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/simple_arithmetic",
            "value": 312652,
            "range": "± 41085",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/conditional",
            "value": 455261,
            "range": "± 69806",
            "unit": "ns/iter"
          },
          {
            "name": "compilation/nested_arithmetic",
            "value": 419142,
            "range": "± 59482",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/int_add",
            "value": 574,
            "range": "± 8",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/mixed_arithmetic",
            "value": 429,
            "range": "± 11",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/comparison",
            "value": 278,
            "range": "± 1",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/cons",
            "value": 1020,
            "range": "± 19",
            "unit": "ns/iter"
          },
          {
            "name": "vm_execution/first",
            "value": 883,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/if_true",
            "value": 568,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "conditionals/nested_if",
            "value": 3045,
            "range": "± 483",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/simple",
            "value": 511384,
            "range": "± 59292",
            "unit": "ns/iter"
          },
          {
            "name": "end_to_end/complex",
            "value": 483504,
            "range": "± 19436",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/10",
            "value": 1759,
            "range": "± 116",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/10",
            "value": 801,
            "range": "± 4",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/50",
            "value": 28457,
            "range": "± 7734",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/50",
            "value": 9387,
            "range": "± 2690",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/100",
            "value": 50383,
            "range": "± 5521",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/100",
            "value": 29640,
            "range": "± 4057",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/list_construction/500",
            "value": 244913,
            "range": "± 40809",
            "unit": "ns/iter"
          },
          {
            "name": "scalability/addition_chain/500",
            "value": 191687,
            "range": "± 32550",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/value_clone",
            "value": 0,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "memory_operations/list_to_vec",
            "value": 108,
            "range": "± 60",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}