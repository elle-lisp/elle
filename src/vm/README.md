# Virtual Machine

The VM executes bytecode produced by the compiler. It's a stack-based machine
with register-addressed local variables.

## Architecture

**Operand Stack**: Most operations push and pop from a central stack. Add pops
two values, adds them, pushes the result.

**Local Variables**: Inside closures, locals live in the closure's environment
vector, accessed by index via `LoadUpvalue`/`StoreUpvalue`. At top level,
locals use `LoadLocal`/`StoreLocal` which access the stack directly.

**Globals**: Stored in a HashMap keyed by `SymbolId`. `LoadGlobal`/`StoreGlobal`
read and write globals.

## Execution Loop

```rust
loop {
    let instr = bytecode[ip];
    ip += 1;
    
    match instr {
        LoadConst => { /* push constant */ }
        Add => { /* pop two, push sum */ }
        Call => { /* complex: setup env, recurse */ }
        Return => { return stack.pop(); }
        // ...
    }
    
    // Check for exceptions after each instruction
    if exception && !handling {
        jump_to_handler_or_propagate();
    }
}
```

## Closure Calls

When calling a closure:

1. Validate arity
2. Build environment: `[captures..., params..., locals...]`
3. Wrap parameters in cells if `cell_params_mask` indicates
4. Push call frame for stack traces
5. Recursively execute closure's bytecode
6. Pop frame, push result

## Tail Calls

Tail calls (`TailCall` instruction) avoid stack growth:

1. Build new environment
2. Store in `pending_tail_call` instead of recursing
3. Return dummy value
4. Outer loop detects pending call, loops back

This enables unbounded recursion without stack overflow.

## Exception Handling

```lisp
(handler-case
  (risky-operation)
  (error e (handle-error e)))
```

Compiles to:
- `PushHandler` with handler offset
- Execute body
- `PopHandler` on success, jump past handlers
- Handler code: `MatchException`, conditionally execute handler

Exceptions propagate by unwinding the stack to `handler.stack_depth` and
jumping to `handler.handler_offset`.

## Coroutines

Coroutines are suspendable computations:

```lisp
(define gen (coroutine (fn () (yield 1) (yield 2) 3)))
(coroutine-resume gen)  ; → 1
(coroutine-resume gen)  ; → 2
(coroutine-resume gen)  ; → 3
```

On `yield`:
1. Create a `ContinuationFrame` capturing IP, stack, env
2. Build `ContinuationData` with the frame chain
3. Mark coroutine as `Suspended`
4. Return `VmResult::Yielded { value, continuation }`

On resume:
1. Replay the continuation frame chain via `resume_continuation`
2. Push resume value (becomes yield's return value)
3. Continue execution

The continuation captures the full call chain from yield point to coroutine
boundary, enabling yield across nested function calls.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/compiler/bytecode.rs` - instruction definitions
- `src/lir/emit.rs` - bytecode emission
