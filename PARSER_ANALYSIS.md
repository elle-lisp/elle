# Elle Parser Implementation Analysis

## Overview
The Elle Lisp interpreter uses a two-stage parsing approach: **Lexer → Reader**.
The parser is located in `/home/adavidoff/git/elle/src/reader.rs` (647 lines).

---

## 1. PARSING STRATEGY

### Architecture: Character-by-Character with Token Buffering
```
Input String
    ↓
Lexer (char-by-char iteration)
    ↓ 
Token Stream (Vec<Token>)
    ↓
Reader (token-by-token with lookahead)
    ↓
Value (S-expression)
```

**Why two-stage?**
- **Separation of concerns**: Lexer handles character-level details, Reader handles grammar
- **Token reusability**: Same tokens could be consumed by different readers/analyzers
- **Simpler debugging**: Can inspect token stream before parsing

### Lexer Implementation
**File**: `/home/adavidoff/git/elle/src/reader.rs` (lines 45-355)

**Key Data Structures**:
```rust
pub struct Lexer {
    input: Vec<char>,           // Entire input as char array (conversion at line 55)
    pos: usize,                 // Current position
    line: usize,                // For error reporting
    col: usize,                 // For error reporting
}
```

**Character Processing**:
- `current()`: Get char at `pos` (O(1) vec indexing)
- `advance()`: Move to next char, updates line/col tracking
- `peek(offset)`: Look ahead without advancing
- Input is **converted to Vec<char> upfront** (line 55: `input.chars().collect()`)

**Performance Characteristics**:
- ✓ Fast random access via vec indexing
- ✗ **PAIN POINT**: Full string→Vec<char> conversion for every parse
  - Allocates memory proportional to input length
  - Unicode chars are pre-validated but stored as 4-byte units
  - Better for interactive REPL but wasteful for large files

### Token Enum
```rust
pub enum Token {
    // Structure: Parens, Brackets, Braces
    LeftParen, RightParen,
    LeftBracket, RightBracket,
    LeftBrace, RightBrace,
    
    // Quoting
    Quote, Quasiquote, Unquote, UnquoteSplicing,
    ListSugar,  // @ for list sugar
    
    // Data
    Symbol(String),
    Keyword(String),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}
```

---

## 2. TOKENIZATION PROCESS

### Whitespace & Comments
```rust
fn skip_whitespace(&mut self) {
    while let Some(c) = self.current() {
        if c.is_whitespace() {
            self.advance();
        } else if c == ';' {
            // Skip comment until newline (lines 94-98)
            while let Some(c) = self.advance() {
                if c == '\n' { break; }
            }
        } else {
            break;
        }
    }
}
```
- Whitespace: Standard `char::is_whitespace()` check
- Comments: `;` to end of line

### Special Cases

#### Numbers (lines 136-162)
```rust
fn read_number(&mut self) -> Result<Token, String> {
    let mut num = String::new();
    let mut has_dot = false;
    
    while let Some(c) = self.current() {
        if c.is_ascii_digit() || c == '-' || c == '+' {
            num.push(c);
            self.advance();
        } else if c == '.' && !has_dot {
            has_dot = true;
            num.push(c);
            self.advance();
        } else {
            break;
        }
    }
    
    if has_dot {
        num.parse::<f64>().map(Token::Float)
    } else {
        num.parse::<i64>().map(Token::Integer)
    }
}
```

**Issue with `-` and `+`** (lines 292-312):
- Must distinguish between sign and symbol
- Current logic: `-` is a sign only if followed by digit or `.`
- Otherwise `-` or `+` alone are symbols
- ✗ **PAIN POINT**: Extra lookahead logic for sign disambiguation

#### Strings (lines 105-134)
```rust
fn read_string(&mut self) -> Result<String, String> {
    self.advance(); // skip opening quote
    let mut s = String::new();
    loop {
        match self.current() {
            None => return Err("Unterminated string"),
            Some('"') => {
                self.advance();
                return Ok(s);
            }
            Some('\\') => {
                self.advance();
                match self.current() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(c) => s.push(c),
                    None => return Err("Unterminated string escape"),
                }
                self.advance();
            }
            Some(c) => {
                s.push(c);
                self.advance();
            }
        }
    }
}
```
- Simple escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`
- Accumulates into String during reading (O(n) for string length)

#### Symbols (lines 164-174)
```rust
fn read_symbol(&mut self) -> String {
    let mut sym = String::new();
    while let Some(c) = self.current() {
        if c.is_whitespace() || "()[]{}'`,:@".contains(c) {
            break;
        }
        sym.push(c);
        self.advance();
    }
    sym
}
```
- Reads until hitting whitespace or special delimiter
- **Delimiter set**: `()[]{}'` , `:@` (quote, quasiquote, unquote, colon, at)
- ✗ **PAIN POINT**: Linear string scan for delimiter check with `contains(c)`

#### Module-Qualified Symbols (lines 176-187)
```rust
fn parse_qualified_symbol(sym: &str) -> (String, String) {
    if let Some(colon_pos) = sym.rfind(':') {
        let module = sym[..colon_pos].to_string();
        let name = sym[colon_pos + 1..].to_string();
        if !module.is_empty() && !name.is_empty() {
            return (module, name);
        }
    }
    (sym.to_string(), String::new())
}
```
- Represents `module:symbol` as a qualified reference
- Used during Reader phase to convert to `(qualified-ref module symbol)`

### Tokenization Entry Point
```rust
pub fn next_token_with_loc(&mut self) -> Result<Option<TokenWithLoc>, String>
```
- Returns `None` at EOF
- Includes `SourceLoc` (line, col) for error reporting
- Also tracks location as each token is consumed

---

## 3. LIST CONSTRUCTION

### List Building Strategy: Rev-Fold Pattern

Lists are built by **reversing elements and folding with cons cells**:

```rust
// From reader.rs lines 523-526 (read_list)
return Ok(elements
    .into_iter()
    .rev()
    .fold(Value::Nil, |acc, v| cons(v, acc)));
```

This creates a proper linked list of cons cells:
```
(1 2 3) →
Cons(1, Cons(2, Cons(3, Nil)))
```

### Cons Cell Structure (value.rs, lines 48-58)
```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Cons {
    pub first: Value,      // CAR
    pub rest: Value,       // CDR (next cons or Nil)
}

#[inline]
pub fn cons(first: Value, rest: Value) -> Value {
    Value::Cons(Rc::new(Cons::new(first, rest)))
}
```

**Memory Layout**:
- Each cons cell is wrapped in `Rc<Cons>` (reference-counted)
- Enables sharing of list tails (structural sharing)
- Each clone is O(1) (refcount increment)

### List Types in Elle

#### Proper Lists (lines 282-291)
```rust
pub fn is_list(&self) -> bool {
    let mut current = self;
    loop {
        match current {
            Value::Nil => return true,
            Value::Cons(cons) => current = &cons.rest,
            _ => return false,
        }
    }
}
```
- Ends with `Nil`
- Linear time check

#### Vectors (distinct from lists)
```rust
pub enum Value {
    Vector(Rc<Vec<Value>>),  // Immutable vector
    // ...
}
```
- Used for `[...]` syntax
- Fixed-size, random access
- Wrapped in `Rc` for sharing

#### Tables (mutable)
```rust
Table(Rc<RefCell<BTreeMap<TableKey, Value>>>)
```
- Key-value store with mutable borrow
- Uses `BTreeMap` for ordered iteration

#### Structs (immutable)
```rust
Struct(Rc<BTreeMap<TableKey, Value>>)
```
- Immutable key-value pairs

### List Construction Variants

#### 1. Regular List `(1 2 3)` (lines 514-531)
```rust
fn read_list(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
    self.advance(); // skip (
    let mut elements = Vec::new();
    
    loop {
        match self.current() {
            None => return Err("Unterminated list"),
            Some(Token::RightParen) => {
                self.advance();
                return Ok(elements
                    .into_iter()
                    .rev()
                    .fold(Value::Nil, |acc, v| cons(v, acc)));
            }
            _ => elements.push(self.read(symbols)?),
        }
    }
}
```

#### 2. Vector `[1 2 3]` (lines 533-547)
```rust
fn read_vector(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
    self.advance(); // skip [
    let mut elements = Vec::new();
    
    loop {
        match self.current() {
            None => return Err("Unterminated vector"),
            Some(Token::RightBracket) => {
                self.advance();
                return Ok(Value::Vector(Rc::new(elements)));
            }
            _ => elements.push(self.read(symbols)?),
        }
    }
}
```

#### 3. List Sugar `@[1 2 3]` → `(list 1 2 3)` (lines 390-419)
- Syntactic sugar for list construction
- Expands to `(list ...)` call during parsing

#### 4. Struct `{k1 v1 k2 v2}` (lines 549-569)
- Expands to `(struct k1 v1 k2 v2)`

#### 5. Table `@{k1 v1 k2 v2}` (lines 571-591)
- Expands to `(table k1 v1 k2 v2)`

### Performance of List Construction

✓ **Good**:
- Parsing is single-pass (O(n) in input length)
- Cons cells use structural sharing via `Rc`
- No copying during fold

✗ **Pain Points**:
- Elements collected in `Vec` first, then reversed: **O(n) space overhead**
- Each `cons()` call allocates new `Rc<Cons>`: **O(n) allocations**
- Converting lists back to Vec (line 294-307) requires traversal: **O(n) time**

---

## 4. VALUE ENUM & REPRESENTATION

### Complete Value Definition (value.rs, lines 142-163)
```rust
#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(SymbolId),              // Interned symbol ID
    Keyword(SymbolId),             // Keywords like :name
    String(Rc<str>),               // Reference-counted string
    Cons(Rc<Cons>),                // Linked list cell
    Vector(Rc<Vec<Value>>),        // Immutable vector
    Table(Rc<RefCell<BTreeMap<TableKey, Value>>>),   // Mutable map
    Struct(Rc<BTreeMap<TableKey, Value>>),           // Immutable map
    Closure(Rc<Closure>),          // Function with captures
    NativeFn(NativeFn),            // Built-in function
    // FFI
    LibHandle(LibHandle),          // Loaded C library handle
    CHandle(CHandle),              // C object pointer
    // Error handling
    Exception(Rc<Exception>),      // Exception value
}
```

### Memory Layout Analysis
| Variant | Size | Notes |
|---------|------|-------|
| Nil | 0 bytes | Empty variant |
| Bool, Int, Float | 1-8 bytes | Inline |
| Symbol, Keyword | 4 bytes | u32 ID |
| String | 8 bytes | Rc<str> (ptr) |
| Cons | 8 bytes | Rc pointer |
| Vector | 8 bytes | Rc pointer |
| Table | 8 bytes | Rc pointer |
| Struct | 8 bytes | Rc pointer |
| Closure | 8 bytes | Rc pointer |
| NativeFn | 8 bytes | Function pointer |
| LibHandle, CHandle | 4-8 bytes | ID/pointer |
| Exception | 8 bytes | Rc pointer |

**Enum size**: ~24 bytes on 64-bit (discriminant + largest variant)

### Helper Functions
```rust
// List construction (line 409-414)
pub fn list(values: Vec<Value>) -> Value {
    values
        .into_iter()
        .rev()
        .fold(Value::Nil, |acc, v| Value::Cons(Rc::new(Cons::new(v, acc))))
}

// Cons cell constructor (line 417-420)
#[inline]
pub fn cons(first: Value, rest: Value) -> Value {
    Value::Cons(Rc::new(Cons::new(first, rest)))
}
```

---

## 5. SYMBOL TABLE IMPLEMENTATION

### Structure (symbol.rs, lines 30-38)
```rust
#[derive(Debug)]
pub struct SymbolTable {
    map: FxHashMap<String, SymbolId>,  // Name → ID lookup
    names: Vec<String>,                 // ID → Name lookup
    macros: FxHashMap<SymbolId, Rc<MacroDef>>,
    modules: FxHashMap<SymbolId, Rc<ModuleDef>>,
    current_module: Option<SymbolId>,
}
```

### Symbol Interning (lines 51-61)
```rust
pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    
    let id = SymbolId(self.names.len() as u32);
    self.names.push(name.to_string());
    self.map.insert(name.to_string(), id);
    id
}
```

**Process**:
1. Check if symbol exists in `map` (O(1) hash lookup)
2. If new: 
   - Assign ID = current `names.len()`
   - Append to `names` Vec
   - Insert into `map`
3. Return `SymbolId(u32)`

### SymbolId Definition (value.rs, lines 6-11)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);
```
- 32-bit unsigned integer ID
- Implements `Copy`, so it's passed by value (8 bytes)
- **O(1) comparison** (just integer comparison)

### Performance Characteristics

✓ **Good**:
- Lookup: O(1) hash map
- Symbol comparison: O(1) ID comparison
- Repeated interns are cached

✗ **Limitation**:
- **Max 2^32 symbols** (~4 billion)
- Symbol names allocated in `names` Vec - never freed
- String duplication in both `map` (key) and `names` (value)

### Usage in Reader
```rust
// Line 490 in reader.rs
let id = symbols.intern(s);
self.advance();
Ok(Value::Symbol(id))
```

Every symbol reference becomes a `SymbolId` value.

---

## 6. PARSING-RELATED HELPER FUNCTIONS

### Main Entry Point: `read_str` (lines 594-616)
```rust
pub fn read_str(input: &str, symbols: &mut SymbolTable) -> Result<Value, String> {
    // Strip shebang if present
    let input = if input.starts_with("#!") {
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };
    
    let mut lexer = Lexer::new(&input);
    let mut tokens = Vec::new();
    
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    
    if tokens.is_empty() {
        return Err("No input");
    }
    
    let mut reader = Reader::new(tokens);
    reader.read(symbols)
}
```

**Steps**:
1. Handle shebang `#!/...` (for scripts)
2. Lex entire input into token stream
3. Create Reader with tokens
4. Parse and return first value

**Limitation**: Only returns first value (used in REPL for single expressions)

### Reader Structure (lines 357-365)
```rust
pub struct Reader {
    tokens: Vec<Token>,
    pos: usize,
}

impl Reader {
    pub fn new(tokens: Vec<Token>) -> Self {
        Reader { tokens, pos: 0 }
    }
}
```

### Reader Navigation
```rust
fn current(&self) -> Option<&Token> {
    self.tokens.get(self.pos)
}

fn advance(&mut self) -> Option<Token> {
    let token = self.current().cloned();
    self.pos += 1;
    token
}

pub fn try_read(&mut self, symbols: &mut SymbolTable) -> Option<Result<Value, String>> {
    let token = self.current().cloned()?;
    Some(self.read_one(symbols, &token))
}
```

### Quoting Macros (lines 422-445)
```rust
Token::Quote => {
    self.advance();
    let val = self.read(symbols)?;
    let quote_sym = Value::Symbol(symbols.intern("quote"));
    Ok(cons(quote_sym, cons(val, Value::Nil)))
}
// Similar for Quasiquote, Unquote, UnquoteSplicing
```

Converts `'x` into `(quote x)` at parse time.

---

## 7. PERFORMANCE PAIN POINTS & BOTTLENECKS

### Critical Issues

#### 1. **Full String→Vec<char> Conversion** (Line 55)
```rust
pub fn new(input: &str) -> Self {
    Lexer {
        input: input.chars().collect(),  // O(n) allocation + unicode validation
        // ...
    }
}
```
**Impact**: 
- Allocates memory for every character upfront
- Validates UTF-8 which was already validated at string creation
- Waste for large files

**Alternative**: Iterator over bytes or UTF-8 segments

#### 2. **Delimiter String Check in Symbol Reading**
```rust
if c.is_whitespace() || "()[]{}'`,:@".contains(c) {  // Linear string search!
```
**Impact**: O(11) character comparisons per symbol character

**Alternative**: Use character set/bit pattern matching

#### 3. **List Building with Vec Reversal**
```rust
elements.into_iter().rev().fold(...)
```
**Impact**: O(n) space for intermediate Vec, O(1) reversal but O(n) allocations for cons cells

#### 4. **List Traversal for Every Operation**
```rust
pub fn list_to_vec(&self) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut current = self.clone();  // <-- Cloning entire value!
    loop {
        match current {
            Value::Nil => return Ok(result),
            Value::Cons(cons) => {
                result.push(cons.first.clone());  // <-- Cloning every element!
                current = cons.rest.clone();
            }
            _ => return Err("Not a proper list"),
        }
    }
}
```
**Impact**: 
- Every list operation requires full traversal (O(n) time)
- Multiple clones of values
- No cached length

**Examples from primitives/list.rs**:
```rust
pub fn prim_length(args: &[Value]) -> Result<Value, String> {
    let vec = args[0].list_to_vec()?;  // O(n) traversal!
    Ok(Value::Int(vec.len() as i64))
}

pub fn prim_nth(args: &[Value]) -> Result<Value, String> {
    let vec = args[1].list_to_vec()?;  // O(n) traversal to get one element!
    vec.get(index).cloned()
}
```

#### 5. **Reference Counting Overhead**
Every value copy involves refcount operations:
```rust
pub fn clone() -> Self {
    // Each Rc::clone() does atomic refcount increment
    Value::Cons(Rc::clone(&self.cons))
}
```
**Impact**: Heavy use in parameter passing

#### 6. **No Lazy Evaluation**
All arguments evaluated before function call (eager evaluation).
No opportunity to avoid expensive computations.

#### 7. **Symbol Name Duplication**
```rust
map: FxHashMap<String, SymbolId>,  // String stored as key
names: Vec<String>,                 // String stored as value
```
Symbol strings stored twice: once in HashMap keys, once in Vec values.

---

## 8. CURRENT DATA STRUCTURES SUMMARY

| Component | Data Structure | Purpose | Performance |
|-----------|-----------------|---------|-------------|
| Lexer input | `Vec<char>` | Character access | O(1) access but O(n) initial allocation |
| Tokens | `Vec<Token>` | Parsed tokens | O(1) random access |
| Reader pos | `usize` | Current token index | O(1) position tracking |
| Symbol names | `Vec<String>` | ID→Name mapping | O(1) lookup, no deletion |
| Symbol map | `FxHashMap<String, SymbolId>` | Name→ID mapping | O(1) avg lookup |
| Macros | `FxHashMap<SymbolId, Rc<MacroDef>>` | Macro definitions | O(1) lookup |
| Modules | `FxHashMap<SymbolId, Rc<ModuleDef>>` | Module definitions | O(1) lookup |
| Lists | `Rc<Cons>` linked list | Cons cells | O(n) traversal, O(1) clone |
| Vectors | `Rc<Vec<Value>>` | Fixed-size arrays | O(1) access, O(n) clone |
| Tables | `Rc<RefCell<BTreeMap>>` | Mutable maps | O(log n) operations |
| Structs | `Rc<BTreeMap>` | Immutable maps | O(log n) operations |

---

## 9. BENCHMARKS INSIGHTS

From `/home/adavidoff/git/elle/benches/benchmarks.rs`:

**Key test cases**:
- `simple_number`: Baseline
- `list_literal`: Parse `(1 2 3 4 5)`
- `nested_expr`: Parse `(+ (* 2 3) (- 10 5))`
- `large_list_100`: Parse `(0 1 2 ... 99)`
- `symbol_interning`: Intern rate
- `list_to_vec`: Traversal cost

**Performance priorities**:
1. Parsing speed (interactive REPL)
2. Symbol interning (repeated use)
3. VM execution
4. Memory operations

---

## 10. SUMMARY OF ARCHITECTURE

```
Source Code
    ↓
Lexer::next_token() [char-by-char]
    ↓
Vec<Token> [all tokens collected]
    ↓
Reader::read() [token-by-token]
    ↓
Value [S-expression, cons-based lists]
    ↓
Compiler::value_to_expr() [convert to AST]
    ↓
compile() [generate bytecode]
    ↓
VM::execute() [run bytecode]
```

The parser is:
- ✓ **Simple**: Clear separation between lexing and reading
- ✓ **Correct**: Proper handling of all S-expression types
- ✓ **Functional**: Works well for interactive use
- ✗ **Not optimized**: Multiple copies, unnecessary allocations, list traversals

