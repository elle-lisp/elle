# tests/common

<!-- audited: 2026-11-13 -->

Shared test helpers for the Elle test suite.

## Responsibility

Hold one copy of each helper the test files share, so that no test file keeps
a variant of its own:

- an eval over a fresh `Runtime`, with or without stdlib, scheduled or not
- an eval over a cached `RuntimeCore`, for property tests
- the proptest configuration that honors `PROPTEST_CASES`
- a scratch directory that is removed on drop
- the readers of the Makefile, the two Elle suites, the suite passes a target
  runs, the CI workflow files, the documents `make doctest` runs, and an
  `elle test` session store

The test harness runs the tests, each test file defines its own cases, and
`tests/fixtures/` holds the static files the tests read.

## Where a helper is described

Each helper carries its own docstring, and each file opens with what it holds.
[mod.rs](mod.rs) opens with the rule every eval follows: inspect a result inside
the closure the eval hands it to, while its heap is alive.
[The tests guide](../AGENTS.md) names the helpers a new test reaches for first.

## Files

| File | Content |
|------|---------|
| [mod.rs](mod.rs) | the evals (`eval_source`, `eval_source_bare`, `eval_source_unscheduled`, `eval_reuse`, `eval_reuse_bare`), `setup`, `proptest_cases`, `paint_stack`, and `ScratchDir`; it re-exports every reader below but those of [documents.rs](documents.rs) |
| [repo.rs](repo.rs) | the Makefile readers (`make_var`, `make_dry_run`, `make_dry_run_with`, `make_expand`, `makefile`), `run_make`, which runs a target, `plugin_package`, the package a `plugins/` directory declares, the suite readers (`repo_root`, `suite`, `suite_files`, `declared_deadline`, `wide_patterns`), and `source_files`, the tree walk a policy sweep reads |
| [makefile.rs](makefile.rs) | the Makefile's rules (`makefile_rules`), the targets whose recipe runs one suite pass (`suite_targets`), and every target that reaches one (`recording_targets`) |
| [passes.rs](passes.rs) | the suite passes a target runs (`Pass`, `passes`, `the_pass`), what `make smoke` names (`smoke_passes`), the suites and the passes as sets (`lang_files`, `impl_files`, `producer_files`, `build_producer_files`, `charge_files`, `isolated_impl_files`), and the assertions on a pass (`assert_plain_language_pass`, `assert_producer_pass`, `assert_rig_runs`) |
| [workflows.rs](workflows.rs) | the workflow readers (`workflow_files`, `workflow_jobs`, `job_steps`, `step_lines`, `runs_target`, `make_steps`) |
| [documents.rs](documents.rs) | the documents `make doctest` runs (`doctest_documents`), the documents it must run (`covered_documents`), and the walk both read (`documents_under`) |
| [store.rs](store.rs) | the session-store readers (`query`, `scalar`), for the tests that drive `elle test` and read back what it recorded |
