# Tools

<!-- audited: 2026-10-20 -->

Programs that help develop Elle and are not part of Elle or of its suites.

The index beside this file lists each tool that has a document of its own. The
three tools below have none.

[aws](aws/aws-gen.lisp) fetches the AWS Smithy models and generates the service
modules of the AWS client from them. The client's
[implementor's guide](../lib/aws/AGENTS.md) describes the generator.

[budget.lisp](budget.lisp) prints the curve [the cap report](../docs/analysis/budget.md)
reads the source cap off. Each row is a bin of sources by tokens, or by lines,
with the count of sources in it, how many name three governing documents, and
that share. It runs on the rig, which counts the tokens. `make budget` builds
the release rig and runs the tool there, whatever build `ELLE` names, because a
debug rig takes minutes to price every source.

[flipctx.py](flipctx.py) is a codemod that added the `NativeCtx` parameter to
each primitive function the compiler named. It reads its repository from a
fixed path, so it does not run here as it stands.
