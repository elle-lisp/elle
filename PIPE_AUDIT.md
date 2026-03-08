# Elle Codebase Audit: `|` Character Usage

## Executive Summary

If `|` becomes a proper delimiter (like `(`, `[`, `{`), **significant breaking changes** are required:

1. **Or-patterns in match expressions** — 5 test cases use `(pattern1 | pattern2 | ...)` syntax
2. **Symbol names** — `|` is currently allowed mid-symbol (e.g., `foo|bar`); making it a delimiter breaks this
3. **Parser changes** — The lexer must add `|` to the delimiter set; the parser must handle it specially
4. **Syntax tree changes** — No new `SyntaxKind` variant needed if or-patterns remain as lists with `|` symbols
5. **Tokenization walkthrough** — Shows how `(1 | 3 | 5)` would tokenize differently

---

## 1. Or-Patterns in Match Expressions

### Current Usage

**File: `tests/elle/matching.lisp`** (5 occurrences)

```lisp
Line 45: (assert-eq (match 1 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :odd
Line 47: (assert-eq (match 2 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
Line 49: (assert-eq (match 0 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
Line 51: (assert-eq (match 9 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :odd
Line 53: (assert-eq (match 4 ((1 | 3 | 5 | 7 | 9) :odd) ((0 | 2 | 4 | 6 | 8) :even) (_ :out)) :even
```

### How Or-Patterns Currently Work

**Rust Implementation: `src/hir/analyze/special.rs:312`**

```rust
let groups: Vec<&[Syntax]> = items.split(|s| s.as_symbol() == Some("|")).collect();
```

The analyzer:
1. Receives a list of syntax items: `[1, |, 3, |, 5, |, 7, |, 9]`
2. Splits on items where `as_symbol() == Some("|")` — i.e., where the item is a symbol with value `"|"`
3. Produces groups: `[[1], [3], [5], [7], [9]]`
4. Validates each group has exactly one pattern
5. Creates `HirPattern::Or(patterns)` with all alternatives

**Key invariant (from `src/hir/pattern.rs:241`):**
All alternatives in an or-pattern must bind the same set of variables.

### Current Tokenization

With `|` as a **symbol** (not a delimiter):

```
Input:  (1 | 3 | 5)
Tokens: LeftParen, Integer(1), Symbol("|"), Integer(3), Symbol("|"), Integer(5), RightParen
Syntax: List([Int(1), Symbol("|"), Int(3), Symbol("|"), Int(5)])
```

The parser produces a flat list. The analyzer then splits on the `|` symbols.

---

## 2. `|` as Part of Symbol Names

### Current Behavior

`|` is **currently allowed in the middle of symbol names**. The lexer does not treat it as a delimiter.

**Lexer: `src/reader/lexer.rs:6-10`**

```rust
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@' | ';'
    )
}
```

Notice: `|` is **NOT** in this list. Therefore, `foo|bar` is lexed as a single symbol token.

### Actual Usage in Codebase

**Test: `foo|bar` symbol**

```bash
$ cat > /tmp/test.lisp << 'EOF'
(def foo|bar 42)
(print foo|bar)
EOF
$ ./target/debug/elle /tmp/test.lisp
42
```

**Verification:** The symbol `foo|bar` is successfully defined and referenced.

### Search Results

No actual symbols containing `|` were found in the codebase (only in comments and strings):

- `examples/processes.lisp:52` — comment: `:alive | :dead | :error`
- `examples/basics.lisp:177` — comment: `bit/or 12 10) 14 "bit/or"    # 1100 | 1010 = 1110`
- `tests/elle/arithmetic.lisp:142` — comment: `# mod_range: (rem a b) has |result| < b`
- `tests/elle/strings.lisp:203` — string literal: `"one|two|three"`
- `stdlib.lisp:565, 595, 601` — string literals: `"|"` (used in graphviz output generation)

**Conclusion:** While `|` is syntactically allowed in symbol names, **no actual symbols in the codebase use it**. However, user code could rely on this feature.

---

## 3. The Or-Pattern Fix: Syntax Changes Required

### Current Syntax

```lisp
(match x
  ((1 | 3 | 5) :odd)
  ((0 | 2 | 4) :even)
  (_ :out))
```

### If `|` Becomes a Delimiter

The tokenizer would produce:

```
Input:  (1 | 3 | 5)
Tokens: LeftParen, Integer(1), Pipe, Integer(3), Pipe, Integer(5), RightParen
```

The parser would then need to decide: **is `Pipe` a valid token inside a list?**

#### Option A: Reject `Pipe` in Lists (Breaks Or-Patterns)

If the parser treats `Pipe` as an error inside lists:

```
Error: unexpected token Pipe at position 2
```

**Result:** All 5 or-pattern tests fail. The syntax `(1 | 3 | 5)` becomes invalid.

#### Option B: Allow `Pipe` in Lists, Convert to Symbol

If the parser converts `Pipe` tokens back to `Symbol("|")` when inside lists:

```
Tokens: LeftParen, Integer(1), Pipe, Integer(3), Pipe, Integer(5), RightParen
Syntax: List([Int(1), Symbol("|"), Int(3), Symbol("|"), Int(5)])
```

**Result:** Or-patterns continue to work. The analyzer's split logic remains unchanged.

**Cost:** The parser becomes more complex. It must track context (inside list vs. top-level) and convert tokens.

#### Option C: Change Or-Pattern Syntax

Introduce a new syntax for or-patterns that doesn't use `|`:

```lisp
(match x
  ((or 1 3 5) :odd)
  ((or 0 2 4) :even)
  (_ :out))
```

**Cost:** All 5 test cases must be rewritten. User code must migrate.

---

## 4. Tokenization Walkthrough: `(1 | 3 | 5)` as a Delimiter

### Current Tokenization (| as symbol)

```
Input:  "(1 | 3 | 5)"
Lexer:
  pos=0: '(' → LeftParen
  pos=1: '1' → Integer(1)
  pos=2: ' ' → skip
  pos=3: '|' → read_symbol() → Symbol("|")
  pos=4: ' ' → skip
  pos=5: '3' → Integer(3)
  pos=6: ' ' → skip
  pos=7: '|' → read_symbol() → Symbol("|")
  pos=8: ' ' → skip
  pos=9: '5' → Integer(5)
  pos=10: ')' → RightParen

Tokens: [LeftParen, Integer(1), Symbol("|"), Integer(3), Symbol("|"), Integer(5), RightParen]

Parser (read_list):
  Advance past LeftParen
  Loop:
    read() → Integer(1) → add to elements
    read() → Symbol("|") → add to elements
    read() → Integer(3) → add to elements
    read() → Symbol("|") → add to elements
    read() → Integer(5) → add to elements
    current() == RightParen → exit loop
  Advance past RightParen
  Return: List([Int(1), Symbol("|"), Int(3), Symbol("|"), Int(5)])
```

### If `|` Becomes a Delimiter

**Step 1: Update `is_delimiter()` in `src/reader/lexer.rs:6`**

```rust
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@' | ';' | '|'
    )
}
```

**Step 2: Add `Pipe` token variant to `src/reader/token.rs:75`**

```rust
pub enum Token<'a> {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Pipe,  // NEW
    // ... rest
}
```

**Step 3: Add `Pipe` handling in `src/reader/lexer.rs:203`**

```rust
Some('|') => {
    self.advance();
    Ok(Some(TokenWithLoc {
        token: Token::Pipe,
        loc,
        len: self.pos - start_pos,
        byte_offset: start_pos,
    }))
}
```

**Step 4: Tokenization now produces**

```
Input:  "(1 | 3 | 5)"
Tokens: [LeftParen, Integer(1), Pipe, Integer(3), Pipe, Integer(5), RightParen]
```

**Step 5: Parser must handle `Pipe` inside lists**

In `src/reader/syntax.rs:206` (`read_list`):

```rust
fn read_list(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
    self.advance(); // skip (
    let mut elements = Vec::new();

    loop {
        match self.current() {
            None => { /* error */ }
            Some(OwnedToken::RightParen) => {
                // ... return list
            }
            Some(OwnedToken::Pipe) => {
                // Option A: Error
                return Err(format!("{}: unexpected | in list", ...));
                
                // Option B: Convert to symbol
                self.advance();
                elements.push(Syntax::new(SyntaxKind::Symbol("|".to_string()), ...));
                
                // Option C: Treat as special (set literal syntax)
                // ... (see section 5)
            }
            _ => elements.push(self.read()?),
        }
    }
}
```

---

## 5. Set Literal Ambiguity: `|1 2 3|`

### Current State

**There is no set literal syntax in Elle.** The `|...|` syntax does not exist.

**Verification:**
- No `SyntaxKind::Set` or `SyntaxKind::FrozenSet` variant in `src/syntax/mod.rs`
- No `Token::Pipe` in `src/reader/token.rs`
- No set literal tests in `tests/elle/`

### If `|` Becomes a Delimiter and Set Literals Are Added

Suppose we want to add frozenset syntax: `|1 2 3|` (immutable set).

#### Tokenization of `|1 2 3|`

```
Input:  "|1 2 3|"
Tokens: [Pipe, Integer(1), Integer(2), Integer(3), Pipe]
```

#### Parser Ambiguity

When the parser encounters a `Pipe` token at the top level:

```rust
fn read_one(&mut self, token: &OwnedToken, loc: &SourceLoc) -> Result<Syntax, String> {
    match token {
        OwnedToken::LeftParen => self.read_list(loc),
        OwnedToken::LeftBracket => self.read_array(loc),
        OwnedToken::LeftBrace => self.read_struct(loc),
        OwnedToken::Pipe => {
            // Is this a set literal or an error?
            // Need to look ahead to determine
        }
        // ...
    }
}
```

#### Resolution: Lookahead Required

The parser must use lookahead to distinguish:

1. **Set literal:** `|1 2 3|` — `Pipe` followed by elements, then closing `Pipe`
2. **Error:** `|` at top level without matching closing `Pipe`

**Implementation:**

```rust
Some(OwnedToken::Pipe) => {
    let start_loc = loc.clone();
    self.advance(); // skip opening |
    let mut elements = Vec::new();

    loop {
        match self.current() {
            None => {
                return Err(format!(
                    "{}: unterminated set literal (missing closing |)",
                    start_loc.position()
                ));
            }
            Some(OwnedToken::Pipe) => {
                let end_loc = self.current_location();
                self.advance(); // skip closing |
                let span = self.merge_spans(&start_loc, &end_loc, &elements);
                return Ok(Syntax::new(SyntaxKind::Set(elements), span));
            }
            _ => elements.push(self.read()?),
        }
    }
}
```

#### Nested Sets: `||` and `| |`

**Empty frozenset: `||`**

```
Tokens: [Pipe, Pipe]
Parser:
  Encounter first Pipe → enter set literal mode
  Immediately encounter second Pipe → closing delimiter
  Result: Set([])  (empty set)
```

**Frozenset containing a space: `| |`**

```
Tokens: [Pipe, Pipe]  (whitespace is skipped)
Parser:
  Same as above
  Result: Set([])  (empty set, not a set containing a space)
```

**Frozenset containing an empty set: `||1||`**

```
Tokens: [Pipe, Pipe, Integer(1), Pipe, Pipe]
Parser:
  Encounter first Pipe → enter set literal mode
  Encounter second Pipe → closing delimiter
  Result: Set([])  (empty set)
  Remaining tokens: [Integer(1), Pipe, Pipe]
  Error: unexpected token Integer(1)
```

**Correct syntax for nested set: `|(|1|)|`**

```
Tokens: [Pipe, LeftParen, Pipe, Integer(1), Pipe, RightParen, Pipe]
Parser:
  Encounter first Pipe → enter set literal mode
  Encounter LeftParen → read_list()
    Encounter Pipe → enter set literal mode
    Encounter Integer(1)
    Encounter Pipe → closing delimiter
    Result: Set([1])
  Encounter RightParen → end of list
  Encounter Pipe → closing delimiter
  Result: Set([List([Set([1])])])
```

---

## 6. Or-Pattern Inside Set: `(match x (|1 | 2|) ...)`

### Tokenization

```
Input:  "(match x (|1 | 2|) ...)"
Tokens: [LeftParen, Symbol("match"), Symbol("x"), Pipe, Integer(1), Pipe, Integer(2), Pipe, RightParen, ...]
```

### Parser Behavior

When `read_list()` encounters the first `Pipe`:

```
Current token: Pipe
Context: inside list (after "x")
```

**Ambiguity:** Is this:
1. A set literal `|1 | 2|` (frozenset)?
2. An or-pattern `(1 | 2)` (but missing opening paren)?
3. An error?

#### Resolution: Context-Dependent Parsing

The parser must decide based on what follows:

**Case 1: `(|1 2|)` — Set literal inside list**

```
Tokens: [LeftParen, Pipe, Integer(1), Integer(2), Pipe, RightParen]
Parser (read_list):
  Encounter Pipe → ???
  
  Option A: Treat as symbol (convert Pipe to Symbol("|"))
    Result: List([Symbol("|"), Int(1), Int(2), Symbol("|")])
    
  Option B: Treat as set literal start
    Advance past Pipe
    Read elements: [1, 2]
    Encounter Pipe → closing delimiter
    Result: List([Set([1, 2])])
```

**Case 2: `(1 | 2)` — Or-pattern (current syntax)**

```
Tokens: [LeftParen, Integer(1), Pipe, Integer(2), RightParen]
Parser (read_list):
  Encounter Pipe → ???
  
  Option A: Treat as symbol (convert Pipe to Symbol("|"))
    Result: List([Int(1), Symbol("|"), Int(2)])
    Analyzer splits on Symbol("|") → or-pattern
    
  Option B: Treat as set literal start
    Advance past Pipe
    Read elements: [2]
    Encounter RightParen (not Pipe) → error: unterminated set
```

#### Proposed Resolution

**Rule: `Pipe` inside a list is always converted to `Symbol("|")`**

This preserves or-pattern syntax and prevents ambiguity:

```rust
fn read_list(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
    self.advance(); // skip (
    let mut elements = Vec::new();

    loop {
        match self.current() {
            None => { /* error */ }
            Some(OwnedToken::RightParen) => {
                // ... return list
            }
            Some(OwnedToken::Pipe) => {
                // Inside a list, Pipe is always a symbol (for or-patterns)
                self.advance();
                let span = self.source_loc_to_span(loc, loc.col + 1);
                elements.push(Syntax::new(SyntaxKind::Symbol("|".to_string()), span));
            }
            _ => elements.push(self.read()?),
        }
    }
}
```

**Result:**
- `(1 | 2)` → `List([Int(1), Symbol("|"), Int(2)])` → or-pattern ✓
- `(|1 2|)` → `List([Symbol("|"), Int(1), Int(2), Symbol("|")])` → not a set literal (sets only at top level)
- `|1 2|` → `Set([1, 2])` → set literal ✓

---

## 7. Other Uses of `|` in the Codebase

### Comments (Not Affected)

- `examples/processes.lisp:52` — `:alive | :dead | :error` (comment)
- `examples/basics.lisp:177` — `1100 | 1010 = 1110` (comment)
- `tests/elle/arithmetic.lisp:142` — `|result| < b` (comment)

### String Literals (Not Affected)

- `tests/elle/strings.lisp:203` — `"one|two|three"` (string literal)
- `stdlib.lisp:565` — `"|"` (string literal in graphviz escaping)
- `stdlib.lisp:595, 601` — `"|"` (string literals in graphviz output)

### Bit Operations (Not Affected)

- `examples/basics.lisp:177` — `(bit/or 12 10)` (function call, not symbol)
- `examples/basics.lisp:184` — `(bit/or (bit/shl 10 4) 5)` (function call)

**Conclusion:** No code outside of or-patterns and symbol names uses `|` as a symbol.

---

## 8. Ripple Effects: Files That Must Change

### If `|` Becomes a Delimiter

#### Lexer Changes

- **`src/reader/lexer.rs:6`** — Add `|` to `is_delimiter()`
- **`src/reader/lexer.rs:203`** — Add `Pipe` token handling in `next_token_with_loc()`

#### Token Changes

- **`src/reader/token.rs:75`** — Add `Pipe` variant to `Token` enum
- **`src/reader/token.rs:99`** — Add `Pipe` variant to `OwnedToken` enum
- **`src/reader/token.rs:121`** — Add `Pipe` arm to `From<Token> for OwnedToken`

#### Parser Changes

- **`src/reader/syntax.rs:206`** — Update `read_list()` to handle `Pipe` tokens
  - Option A: Convert to `Symbol("|")` (preserves or-patterns)
  - Option B: Reject as error
  - Option C: Implement set literal syntax
- **`src/reader/syntax.rs:108`** — Update `read_one()` to handle top-level `Pipe` (if set literals added)

#### Syntax Changes (If Set Literals Added)

- **`src/syntax/mod.rs`** — Add `SyntaxKind::Set(Vec<Syntax>)` variant
- **`src/syntax/display.rs`** — Add display arm for `SyntaxKind::Set`
- **`src/syntax/convert.rs`** — Add conversion arms for `SyntaxKind::Set`

#### HIR Changes (If Set Literals Added)

- **`src/hir/analyze/forms.rs`** — Add `SyntaxKind::Set` analysis
- **`src/hir/expr.rs`** — Add `HirKind::Set` variant (if needed)

#### Value Changes (If Set Literals Added)

- **`src/value/heap.rs`** — Add `HeapObject::Set` variant
- **`src/value/repr/constructors.rs`** — Add `Value::set()` constructor
- **`src/value/repr/accessors.rs`** — Add `is_set()` predicate
- **`src/value/display.rs`** — Add display arm for sets
- **`src/primitives/types.rs`** — Add `set?` predicate function

#### Test Changes

- **`tests/elle/matching.lisp`** — 5 or-pattern tests (lines 45, 47, 49, 51, 53)
  - If or-patterns are preserved: no changes needed
  - If or-pattern syntax changes: all 5 tests must be rewritten

---

## 9. Summary Table: Impact Assessment

| Aspect | Current | If `|` Becomes Delimiter | Effort |
|--------|---------|-------------------------|--------|
| **Or-patterns** | `(1 \| 3 \| 5)` works | Requires parser context handling or syntax change | Medium |
| **Symbol names** | `foo\|bar` allowed | Would break if not handled | Low (convert to symbol in lists) |
| **Set literals** | Not supported | Could be added with `\|1 2 3\|` syntax | High (new type, new instructions) |
| **Lexer** | 1 delimiter check | Add `\|` to `is_delimiter()` | Low |
| **Token enum** | 13 variants | Add `Pipe` variant | Low |
| **Parser** | Flat list parsing | Context-aware parsing (lists vs. top-level) | Medium |
| **Tests** | 5 or-pattern tests | May need rewriting if syntax changes | Low-Medium |
| **Backward compatibility** | N/A | Breaking change for symbol names | High |

---

## 10. Recommendations

### Option 1: Preserve Or-Patterns (Recommended)

**Approach:** Make `|` a delimiter, but convert it to `Symbol("|")` inside lists.

**Pros:**
- Or-patterns continue to work without syntax changes
- Minimal test changes
- Allows future set literal syntax at top level

**Cons:**
- Parser becomes context-aware (slightly more complex)
- Symbol names with `|` no longer work (breaking change, but no actual usage found)

**Implementation:**
1. Add `|` to `is_delimiter()` in lexer
2. Add `Pipe` token variant
3. Update `read_list()` to convert `Pipe` → `Symbol("|")`
4. No changes to analyzer or HIR

### Option 2: Change Or-Pattern Syntax

**Approach:** Introduce `(or pattern1 pattern2 ...)` syntax instead of `(pattern1 | pattern2 ...)`.

**Pros:**
- Cleaner separation of concerns
- Allows `|` to be used for set literals without ambiguity

**Cons:**
- Breaking change for all 5 or-pattern tests
- User code must migrate
- More verbose syntax

**Implementation:**
1. Add `|` to `is_delimiter()` in lexer
2. Add `Pipe` token variant
3. Update analyzer to recognize `(or ...)` form
4. Rewrite 5 test cases

### Option 3: Don't Make `|` a Delimiter

**Approach:** Keep `|` as a symbol, don't add set literal syntax.

**Pros:**
- No breaking changes
- Or-patterns continue to work
- Symbol names with `|` continue to work

**Cons:**
- Can't add set literal syntax later
- Limits language expressiveness

**Implementation:**
- No changes needed

---

## Conclusion

Making `|` a proper delimiter is **feasible but requires careful design**:

1. **Or-patterns** can be preserved by converting `Pipe` tokens to `Symbol("|")` inside lists
2. **Symbol names** with `|` would break unless handled specially
3. **Set literals** could be added with `|...|` syntax at the top level
4. **5 test cases** in `tests/elle/matching.lisp` would continue to work if or-patterns are preserved

The **recommended approach** is Option 1: make `|` a delimiter, convert it to a symbol inside lists, and preserve or-pattern syntax. This allows future set literal syntax while maintaining backward compatibility for or-patterns.
