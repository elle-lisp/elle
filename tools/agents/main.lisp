(elle/epoch 18)
# audited: 2026-11-23
# Builds each directory's AGENTS.md from its documents' call-outs, or checks or counts the committed indexes.
# tools/agents/overview.md
#
#   elle-rig tools/agents/main.lisp [--root DIR] [--print DIR | --check [DIR] | --counts | DIR]
#
# `make agents` and `make agents-check` run this from the repository root, the
# root unless --root names another. The import names the file beside this one.

(def agents ((import "./agents")))
(def r (agents:run (sys/args)))
(print (get r :stdout))
(eprint (get r :stderr))
(sys/exit (get r :exit))
