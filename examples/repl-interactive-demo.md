# Elle REPL with Readline Support

This document demonstrates the enhanced REPL features with readline support.

## Features

The Elle REPL now includes:

1. **Command History** - Previous commands are saved to `~/.elle_history`
2. **Line Editing** - Use arrow keys to move through lines
3. **Ctrl+A/Ctrl+E** - Jump to beginning/end of line
4. **Ctrl+R** - Search through history
5. **Ctrl+C** - Interrupt current command
6. **Ctrl+D** - Exit REPL (EOF)

## Starting the REPL

```bash
$ cargo run --release
Elle v0.1.0 - Lisp Interpreter (type (help) for commands)
> 
```

## Interactive Session Example

```lisp
; Simple arithmetic
> (+ 1 2 3)
⟹ 6

; List operations
> (list:length '(a b c d e))
⟹ 5

; Define a function
> (define (square x) (* x x))

; Use the function
> (square 5)
⟹ 25

; Pattern matching
> (match '(1 2 3)
    [(list:cons x rest) (+ x (list:length rest))])
⟹ 4

; String operations
> (string:upcase "hello")
⟹ "HELLO"

; Complex expressions
> (define (factorial n)
    (if (<= n 1)
      1
      (* n (factorial (- n 1)))))
> (factorial 5)
⟹ 120

; View help
> (help)

; Exit REPL
> (exit)
```

## History Usage

After running the REPL, your command history is saved in `~/.elle_history`:

```bash
$ cat ~/.elle_history
(+ 1 2 3)
(list:length '(a b c d e))
(define (square x) (* x x))
(square 5)
...
```

## Readline Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| ↑ / ↓ | Navigate command history |
| ← / → | Move cursor left/right |
| Ctrl+A | Jump to beginning of line |
| Ctrl+E | Jump to end of line |
| Ctrl+K | Delete from cursor to end of line |
| Ctrl+U | Delete from start of line to cursor |
| Ctrl+R | Search history (reverse-i-search) |
| Ctrl+C | Cancel current command |
| Ctrl+D | Exit REPL |
| Tab | (Future: Auto-completion) |

## Multi-line Expressions

Readline allows you to enter multi-line expressions naturally:

```lisp
> (define (complex-function x y z)
    (if (> x 0)
      (* y z x)
      (+ x y z)))
```

Just keep typing and the REPL will wait for you to close all parentheses.

## Fallback Behavior

If readline initialization fails (e.g., on unsupported terminals), the REPL will automatically fall back to basic stdin input:

```
✗ Failed to initialize readline: ...
Using fallback stdin input (no history or editing)
> 
```

This ensures Elle works on all platforms while providing enhanced features where available.

## Benefits Over Previous REPL

### Before (Basic stdin)
- No command history between sessions
- Limited line editing
- Difficult to recover from typos
- No search through history

### After (Readline-enabled)
- ✓ Command history persisted to disk
- ✓ Full line editing with arrow keys
- ✓ Quick history search with Ctrl+R
- ✓ Better terminal experience
- ✓ Cross-platform support
- ✓ Automatic fallback on unsupported terminals

## Example Session Transcript

```
$ cargo run --release
Elle v0.1.0 - Lisp Interpreter (type (help) for commands)
> (define numbers '(10 20 30 40 50))
> (list:length numbers)
⟹ 5
> (define doubled (fn [x] (* x 2)))
> (doubled 21)
⟹ 42
> ; Use Ctrl+R to search history
> (list:length numbers)  ; Found and executed from history
⟹ 5
> (exit)

Goodbye!
$
```

## Implementation Details

The readline support is provided by the `rustyline` crate, which:

- Provides cross-platform line editing (Windows, macOS, Linux)
- Handles history persistence automatically
- Manages signal handling (Ctrl+C, Ctrl+D)
- Supports custom completion (extensible for future use)

The implementation is in `src/repl.rs` and wraps rustyline's `DefaultEditor` to provide a clean interface for the REPL.
