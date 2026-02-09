# Elle Parser Architecture - Quick Reference

## 1. PARSING PIPELINE

```
Source Code
    |
    v
Lexer (reader.rs:45-355)
    - Input: String → Vec<char> (PAIN POINT)
    - Char-by-char iteration with peek
    - Produces: Vec<Token>
    |
    v
Reader (reader.rs:357-591)
    - Input: Vec<Token>
    - Token-by-token consumption
    - Produces: Value (S-expression)
    |
    v
Value (cons-based linked lists)
    |
    v
Compiler → Bytecode → VM Execution
```

## 2. KEY COMPONENTS

### Lexer (lines 45-355)
```
pub struct Lexer {
    input: Vec<char>,     // O(n) upfront allocation
    pos: usize,
    line: usize,          // Error reporting
    col: usize,           // Error reporting
}
```

**Main Methods:**
- `next_token()` → `Option<Token>`
- `next_token_with_loc()` → `Option<TokenWithLoc>`

### Token Enum (lines 24-43)
```
LeftParen, RightParen,          // ( )
LeftBracket, RightBracket,      // [ ]
LeftBrace, RightBrace,          // { }
Quote, Quasiquote,              // ' `
Unquote, UnquoteSplicing,       // , ,@
ListSugar,                       // @
Symbol(String),                 // Symbols
Keyword(String),                // Keywords (:name)
Integer(i64), Float(f64),       // Numbers
String(String),                 // Strings
Bool(bool), Nil                 // Literals
```

### Reader (lines 357-591)
```
pub struct Reader {
    tokens: Vec<Token>,
    pos: usize,
}
```

**Main Methods:**
- `read()` → `Result<Value>`
- `try_read()` → `Option<Result<Value>>`
- `read_one()` → `Result<Value>` (internal dispatch)

### Value Enum (lines 142-163 in value.rs)
```
Nil, Bool, Int, Float,         // Scalars
Symbol(SymbolId),              // Interned symbols (u32)
Keyword(SymbolId),             // Keywords
String(Rc<str>),               // Reference-counted strings
Cons(Rc<Cons>),                // Linked list cells
Vector(Rc<Vec<Value>>),        // Immutable vectors
Table(...), Struct(...),       // Maps
Closure(...), NativeFn(...),   // Functions
LibHandle(...), CHandle(...),  // FFI
Exception(...)                 // Error handling
```

### SymbolTable (symbol.rs)
```
pub struct SymbolTable {
    map: FxHashMap<String, SymbolId>,      // Name → u32
    names: Vec<String>,                     // u32 → Name
    macros: FxHashMap<SymbolId, Rc<MacroDef>>,
    modules: FxHashMap<SymbolId, Rc<ModuleDef>>,
}

pub fn intern(&mut self, name: &str) -> SymbolId
    - O(1) lookup if cached
    - O(1) insert if new
    - Returns u32 ID
```

## 3. TOKENIZATION STRATEGY

### Whitespace & Comments (lines 88-103)
- Whitespace: `char::is_whitespace()`
- Comments: `;` to end of line

### Numbers (lines 136-162)
- Integer: `i64::parse()`
- Float: `f64::parse()`
- Sign handling: Lookahead check

### Strings (lines 105-134)
- Escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`
- Accumulation in String

### Symbols (lines 164-174)
- Read until: whitespace or `"()[]{}'` , `:@"`
- **PAIN POINT**: `contains(c)` linear search

### Module-Qualified Symbols (lines 176-187)
- Format: `module:symbol`
- Converted to: `(qualified-ref module symbol)`

## 4. LIST CONSTRUCTION

### Building Process (lines 514-531)
```rust
fn read_list(&mut self) -> Result<Value> {
    let mut elements = Vec::new();
    loop {
        match self.current() {
            Some(Token::RightParen) => {
                return Ok(elements
                    .into_iter()
                    .rev()                    // ← Reversal
                    .fold(Value::Nil, |acc, v| cons(v, acc)))  // ← Fold with cons
            }
            _ => elements.push(self.read()?)
        }
    }
}
```

### Result Structure
```
(1 2 3) →
Cons(1, Cons(2, Cons(3, Nil)))

Memory:
Rc<Cons> { first: 1, rest: Rc<Cons> { ... } }
```

### Other Structures
- `[1 2 3]` → `Vector(Rc<Vec<Value>>)`
- `{k v}` → `(struct k v)` → `Cons(...)`
- `@[1 2]` → `(list 1 2)` → `Cons(...)`

## 5. SYMBOL INTERNING

### Two-way Mapping
```
String ──map──> SymbolId (u32)
     <──names──

Examples:
"foo"    → SymbolId(0)
"bar"    → SymbolId(1)
"cons"   → SymbolId(2)
```

### Performance
- **Intern hit**: O(1) hash lookup
- **New symbol**: O(1) insert
- **Comparison**: O(1) u32 comparison
- **Limitations**: 2^32 max symbols, no deletion

### Usage
```
read_str("(foo bar)") →
Value::Cons(Rc::new(Cons {
    first: Value::Symbol(SymbolId(0)),  // "foo"
    rest: Value::Cons(Rc::new(Cons {
        first: Value::Symbol(SymbolId(1)), // "bar"
        rest: Value::Nil
    }))
}))
```

## 6. PERFORMANCE CHARACTERISTICS

### Parsing
| Operation | Time | Space | Notes |
|-----------|------|-------|-------|
| Lex token | O(k) | O(k) | k = token length |
| Read simple | O(1) | O(1) | Numbers, strings |
| Read list | O(n) | O(n) | n = element count |
| Intern symbol | O(1) | O(k) | k = string length |

### Lists
| Operation | Time | Space | Clones |
|-----------|------|-------|--------|
| cons() | O(1) | 8B | 0 |
| list(...) | O(n) | O(n) | n |
| list_to_vec() | O(n) | O(n) | 2n |
| length | O(n) | O(n) | 2n |
| nth(i) | O(n) | O(n) | 2n |
| first | O(1) | O(1) | 0 |

### Symbols
| Operation | Time | Notes |
|-----------|------|-------|
| intern hit | O(1) | Hash lookup |
| intern new | O(1) | Hash insert |
| compare | O(1) | u32 comparison |

## 7. TOP PAIN POINTS

### 1. String→Vec<char> Conversion (reader.rs:55)
**Issue**: Allocates new array for every parse
**Impact**: O(n) allocation, UTF-8 re-validation
**Example**:
```rust
input: input.chars().collect()  // ← O(n) allocation
```
**Fix**: Use iterator or byte indexing

### 2. Delimiter Checking (reader.rs:167)
**Issue**: Linear string search per character
**Impact**: O(11) comparisons per symbol character
**Example**:
```rust
"()[]{}'`,:@".contains(c)  // ← Linear search!
```
**Fix**: Use bitset or char array matching

### 3. List Traversal for Operations (value.rs:294-307)
**Issue**: Every list operation requires full traversal
**Impact**: O(n) time, O(n) clones, no length caching
**Example**:
```rust
pub fn list_to_vec(&self) -> Result<Vec<Value>> {
    let mut current = self.clone();  // ← Full clone
    loop {
        // O(n) iterations with cloning
    }
}
```
**Fix**: Cache length, use iterator, avoid cloning

### 4. Symbol Name Duplication
**Issue**: Strings stored twice (HashMap key + Vec value)
**Impact**: 2x memory for symbol strings
**Example**:
```rust
map: FxHashMap<String, SymbolId>,  // Key is String
names: Vec<String>,                 // Value is String
```
**Fix**: Use Rc<str> or store once

### 5. Reference Counting Overhead
**Issue**: Every clone increments refcount atomically
**Impact**: Slower value passing, memory synchronization
**Example**:
```rust
Value::Cons(Rc::clone(&cons))  // ← Atomic operation
```
**Fix**: Use more iterator patterns, reduce clones

### 6. No Length Caching
**Issue**: No cached list length
**Impact**: O(n) for length, O(n) for nth, O(n) for access
**Example**:
```rust
// To get length:
pub fn prim_length(args: &[Value]) -> Result<Value> {
    let vec = args[0].list_to_vec()?;  // ← O(n)!
    Ok(Value::Int(vec.len() as i64))
}
```
**Fix**: Cache length in list structure

### 7. Eager Argument Evaluation
**Issue**: All args evaluated before function
**Impact**: Can't use lazy evaluation or short-circuit ops
**Example**: `(if #f (expensive-calc) 1)` evaluates both branches

## 8. DATA STRUCTURE SIZES (64-bit)

| Type | Size | Notes |
|------|------|-------|
| SymbolId | 4B | u32 |
| Nil | 0B | Empty variant |
| Bool | 1B | + 7B padding |
| Int | 8B | i64 |
| Float | 8B | f64 |
| Symbol/Keyword | 4B | SymbolId |
| String | 8B | Rc pointer |
| Cons | 8B | Rc pointer |
| Vector | 8B | Rc pointer |
| Table/Struct | 8B | Rc pointer |
| Closure | 8B | Rc pointer |
| Value (enum) | ~24B | Discriminant + largest variant |

## 9. ENTRY POINTS

### Main Parsing
```rust
pub fn read_str(input: &str, symbols: &mut SymbolTable) -> Result<Value>
    File: reader.rs, lines 594-616
    
    Flow:
    1. Strip shebang
    2. Create Lexer
    3. Collect all tokens
    4. Create Reader
    5. Parse first value
    6. Return Result<Value>
```

### Token Lexing
```rust
pub fn next_token(&mut self) -> Result<Option<Token>>
    File: reader.rs, lines 351-354
    
    Returns:
    - None if EOF
    - Some(Token) if valid token
    - Err(String) if parse error
```

### Symbol Interning
```rust
pub fn intern(&mut self, name: &str) -> SymbolId
    File: symbol.rs, lines 51-61
    
    Returns:
    - Existing SymbolId if cached
    - New SymbolId if not
```

## 10. COMPLETE FILE STRUCTURE

```
reader.rs [647 lines]
├── Token enum [lines 24-43]
├── Lexer struct [lines 45-355]
│   ├── new() [lines 53-60]
│   ├── next_token_with_loc() [lines 189-349]
│   ├── skip_whitespace() [lines 88-103]
│   ├── read_string() [lines 105-134]
│   ├── read_number() [lines 136-162]
│   ├── read_symbol() [lines 164-174]
│   └── parse_qualified_symbol() [lines 176-187]
├── Reader struct [lines 357-591]
│   ├── new() [lines 362-365]
│   ├── read() [lines 507-512]
│   ├── try_read() [lines 379-382]
│   ├── read_one() [lines 385-505]
│   ├── read_list() [lines 514-531]
│   ├── read_vector() [lines 533-547]
│   ├── read_struct() [lines 549-569]
│   └── read_table() [lines 571-591]
└── read_str() [lines 594-616]

value.rs [442 lines]
├── SymbolId struct [lines 6-11]
├── Cons struct [lines 48-58]
├── Value enum [lines 142-163]
├── cons() helper [lines 417-420]
├── list() helper [lines 409-414]
└── list_to_vec() [lines 294-307]

symbol.rs [139 lines]
├── SymbolTable struct [lines 30-38]
├── intern() [lines 51-61]
├── name() [lines 63-66]
├── get() [lines 68-71]
└── Macro/Module support
```

---

## Quick Test: What happens with `(+ 1 2)`?

```
Input: "(+ 1 2)"
         ^^^^^^^^

1. Lexer converts to Vec<Token>:
   [Token::LeftParen, Token::Symbol("+"), Token::Integer(1), 
    Token::Integer(2), Token::RightParen]

2. Reader consumes tokens:
   - Sees LeftParen → calls read_list()
   - Reads Symbol("+") → interns "+", creates Symbol(id)
   - Reads Integer(1) → creates Int(1)
   - Reads Integer(2) → creates Int(2)
   - Sees RightParen → stops, builds list

3. List construction (reverse-fold):
   elements = [Symbol("+"), Int(1), Int(2)]
   
   Reverse and fold:
   Nil
   → Cons(Int(2), Nil)
   → Cons(Int(1), Cons(Int(2), Nil))
   → Cons(Symbol("+"), Cons(Int(1), Cons(Int(2), Nil)))

4. Final Value:
   Cons(
     first: Symbol(SymbolId(0)),  // "+"
     rest: Cons(
       first: Int(1),
       rest: Cons(
         first: Int(2),
         rest: Nil
       )
     )
   )

5. Compiler converts to AST, then bytecode

6. VM executes the bytecode
```

