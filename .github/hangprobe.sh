#!/usr/bin/env bash
# Evidence for a corpus file that wedges only on a runner.
#
# `elle test` bounds each form with its own deadline, so a wedged form is
# RECORDED and the process moves on — the stacks are gone by the time the tally
# is printed. This runs the file DIRECTLY instead, so a wedge is a live process
# this script can photograph: on macOS with `sample`, elsewhere with whatever
# stack sampler the box has. It then prints the runner's own account of the same
# file for the tail the DB captured.
#
# It never fails the job. `make smoke` is the gate and already fails on the
# timeout; this step exists to say WHERE the program stopped, and a probe that
# could fail on its own would add a second, noisier verdict about the same run.
#
# Usage: hangprobe.sh <file.lisp> [attempts] [seconds-per-attempt]
set -u

file="${1:?usage: hangprobe.sh <file.lisp> [attempts] [seconds]}"
attempts="${2:-5}"
budget="${3:-45}"
elle="${ELLE:-./target/release/elle}"

# The pool backend is what every non-Linux build runs, and on Linux io_uring
# hides it. Probing it explicitly means a Linux runner exercises the same wait
# path a Mac does, so this script is worth running on either.
if [ "$(uname -s)" = "Linux" ]; then
  backend_flag="--no-uring"
else
  backend_flag=""
fi

photograph() { # photograph <pid>
  local pid="$1"
  if command -v sample >/dev/null 2>&1; then
    echo "--- sample $pid (macOS) ---"
    sample "$pid" 3 2>&1 | head -200
  elif command -v eu-stack >/dev/null 2>&1; then
    echo "--- eu-stack $pid ---"
    eu-stack -p "$pid" 2>&1 | head -200
  elif command -v lldb >/dev/null 2>&1; then
    echo "--- lldb $pid ---"
    lldb -p "$pid" --batch -o "thread backtrace all" -o detach 2>&1 | head -200
  else
    echo "--- no stack sampler on this box; pid $pid was alive at the deadline ---"
  fi
}

# The probe is normally read through a pipe (a CI log, `| head`), and a closed
# pipe kills it wherever it happens to be. Clean up from a trap so an interrupted
# run leaves no scratch file behind, not just a run that reaches the end.
out=""
cleanup() { [ -n "$out" ] && rm -f "$out"; }
trap cleanup EXIT INT TERM PIPE

hung=0
for i in $(seq 1 "$attempts"); do
  # BSD mktemp (the Mac's) requires a template, so give one; the TMPDIR
  # fallback keeps the scratch file out of a fixed path either way.
  out="$(mktemp "${TMPDIR:-/tmp}/elle-hangprobe.XXXXXX")"
  # shellcheck disable=SC2086
  "$elle" $backend_flag "$file" >"$out" 2>&1 &
  pid=$!
  waited=0
  while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt "$budget" ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    hung=$((hung + 1))
    echo "=============================================================="
    echo "HANG on attempt $i of $attempts after ${budget}s: $file"
    echo "--- last output before the wedge ---"
    tail -20 "$out"
    photograph "$pid"
    kill -9 "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    echo "=============================================================="
  else
    wait "$pid" 2>/dev/null
  fi
  rm -f "$out"
done

echo "hangprobe: $hung of $attempts direct runs wedged ($file)"

# The runner's own account: whatever it captured for this file most recently,
# including the stdout tail of a form that never returned.
echo "--- elle test, same file ---"
"$elle" test "$file" 2>&1 | tail -5
echo "--- the runner's problem rows for this file ---"
"$elle" test --query "SELECT r.tier, r.status, r.reason FROM result r
  JOIN form f ON f.hash = r.form_hash
  WHERE f.file LIKE '%$(basename "$file")' AND r.status <> 'pass'
  ORDER BY r.rowid DESC LIMIT 20" 2>&1 | tail -25

exit 0
