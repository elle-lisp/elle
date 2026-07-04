// Unit tests for the threadpool backend, split by concern:
//   - `hub`:      CompletionHub in_flight accounting (the one-channel invariant)
//   - `process`:  ProcessWait completion encoding
//   - `openfile`: Open op fd/errno results
//   - `signals`:  forked signalfd/kqueue read + close-time drain regressions
//   - `stdin`:    stdin worker shutdown (idle and mid-read)
mod hub;
mod openfile;
mod process;
mod signals;
mod stdin;
