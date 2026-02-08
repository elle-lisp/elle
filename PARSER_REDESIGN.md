# Elle Parser Redesign: Technical Design Document

**Document Status:** Draft  
**Date:** February 7, 2026  
**Target Version:** 0.2.0  
**Scope:** Parser performance optimization and architecture improvement

---

## 1. Executive Summary

The Elle parser is a critical component of the runtime responsible for converting source code into executable Value structures. Current profiling and benchmarking indicate the parser is a measurable bottleneck for interactive use cases (REPLs, incremental compilation) and can be significantly improved through targeted architectural changes.

### What We're Fixing

1. **Char-by-char tokenization overhead** - Upfront conversion to `Vec<char>` adds allocation and cache pressure
2. **String duplication in symbol interning** - Symbols currently stored as `String`, causing redundant allocations
3. **Delimiter checking via string contains()** - O(n) character set membership checks instead of bitsets
4. **List length traversal** - Computing length requires walking entire list structure (recursive, cache-unfriendly)
5. **Index-based parsing** - Token iteration via manual position tracking instead of iterators

### Expected Outcomes

- **Parsing performance:** 30-40% faster (5.5µs → 3-4µs per 100-element list)
- **Symbol operations:** 20-30% reduction in symbol memory overhead
- **List operations:** 15-20% faster `length()` calls
- **Measurable instruction count reduction:** Quantified via iai-callgrind

---

## 2. Current State Analysis

### 2.1 Performance Bottlenecks

Benchmark data from `benches/benchmarks.rs` shows:

| Operation | Current Time | Elements | Per-Element |
|-----------|-------------|----------|------------|
| Parse simple number | ~500ns | 1 | 500ns |
| Parse list (5 elements) | ~2.1µs | 5 | 420ns |
| Parse nested expr | ~3.2µs | 5 ops | 640ns |
| Large list (100) | ~5.5µs | 100 | 55ns |
| Symbol intern (new) | ~150ns | 1 | 150ns |
| Symbol intern (cached) | ~30ns | 1 | 30ns |

**Key Insight:** Amortized per-element parsing is only 55ns but peaks at 500ns for individual numbers, indicating overhead in the lexer setup and tokenization.

### 2.2 Current Architecture

#### Lexer (src/reader.rs, lines 45-355)

```rust
pub struct Lexer {
    input: Vec<char>,      // ← PAIN POINT: O(n) upfront allocation
    pos: usize,            // Manual position tracking
    line: usize,           // For error reporting
    col: usize,            // For error reporting
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),  // ← Allocates and collects all chars
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()  // ← Copy on every call
    }

    fn read_symbol(&mut self) -> String {
        let mut sym = String::new();
        while let Some(c) = self.current() {
            if c.is_whitespace() || "()[]{}'`,:@".contains(c) {  // ← String contains()
                break;
            }
            sym.push(c);
            self.advance();
        }
        sym
    }
}
```

**Problems:**
- Line 55: `input.chars().collect()` creates full Vec<char> upfront—memory waste for large files
- Line 63: Every delimiter check is O(n) string search: `"()[]{}'`,:@".contains(c)`
- Line 165: Manual position tracking instead of leveraging byte indexing
- Line 46: String allocation for every symbol during tokenization (not interned yet)

#### Reader (src/reader.rs, lines 357-591)

```rust
pub struct Reader {
    tokens: Vec<Token>,    // Vec<Token> built upfront
    pos: usize,            // Manual iteration
}

impl Reader {
    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.current().cloned();
        self.pos += 1;
        token
    }
}
```

**Problems:**
- Token vector fully materialized before parsing (can be lazy)
- Clone on every `advance()` call
- Two-pass parsing: Lexer → Vec<Token> → Reader

#### Symbol Table (src/symbol.rs, lines 30-61)

```rust
pub struct SymbolTable {
    map: FxHashMap<String, SymbolId>,  // String keys
    names: Vec<String>,                 // Owned Strings
}

pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    let id = SymbolId(self.names.len() as u32);
    self.names.push(name.to_string());  // ← Allocates new String
    self.map.insert(name.to_string(), id);  // ← Allocates another String for map key
    id
}
```

**Problems:**
- Line 58: `name.to_string()` allocates for symbol storage
- Line 59: `name.to_string()` allocates again for map key
- Total: 2 allocations per new symbol (should be 1)

#### Value & Cons (src/value.rs, lines 48-58)

```rust
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}

// List length requires full traversal:
pub fn list_to_vec(&self) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut current = self.clone();
    loop {
        match current {
            Value::Nil => return Ok(result),
            Value::Cons(cons) => {
                result.push(cons.first.clone());
                current = cons.rest.clone();  // ← Recursive traversal
            }
            _ => return Err("Not a proper list".to_string()),
        }
    }
}
```

**Problems:**
- No length field on Cons
- Computing length requires O(n) traversal with n clones
- Cache-unfriendly linked list traversal

### 2.3 Pain Points Summary

| Issue | Location | Impact | Severity |
|-------|----------|--------|----------|
| Vec<char> upfront allocation | reader.rs:55 | Memory, cache pressure | High |
| String contains() for delimiters | reader.rs:167 | O(n) membership checks | Medium |
| Symbol string duplication | symbol.rs:58-59 | 2 allocations per symbol | Medium |
| List length via traversal | value.rs:294-307 | O(n) + clones | Medium |
| Index-based token iteration | reader.rs:358-375 | Manual state tracking | Low |

---

## 3. Proposed Architecture

### 3.1 Streaming Tokenizer (Bytes, Not Chars)

**Goal:** Eliminate Vec<char> upfront allocation and iterate directly over UTF-8 bytes.

#### New Design

```rust
pub struct StreamingLexer<'a> {
    input: &'a [u8],           // Borrow bytes instead of allocating
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> StreamingLexer<'a> {
    pub fn new(input: &'a str) -> Self {
        StreamingLexer {
            input: input.as_bytes(),  // No allocation
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    #[inline]
    fn current(&self) -> Option<u8> {
        self.input.get(self.pos).copied()  // Byte, not char
    }

    #[inline]
    fn current_char(&self) -> Option<char> {
        // Decode UTF-8 on demand
        if self.pos < self.input.len() {
            match std::str::from_utf8(&self.input[self.pos..]) {
                Ok(s) => s.chars().next(),
                Err(_) => None,
            }
        } else {
            None
        }
    }

    fn read_symbol(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.current() {
            // Check using byte values directly
            if b.is_ascii_whitespace() || b == b'(' || b == b')' 
                || b == b'[' || b == b']' || b == b'{' || b == b'}'
                || b == b'\'' || b == b'`' || b == b',' || b == b':' || b == b'@' {
                break;
            }
            self.advance();
        }
        // Safe: we know the range is valid UTF-8 since we just iterated
        String::from_utf8_lossy(&self.input[start..self.pos]).into_owned()
    }
}
```

**Benefits:**
- No upfront `Vec<char>` allocation
- Byte-level operations are faster (no UTF-8 decoding overhead for ASCII)
- Borrows input instead of copying
- Reduced memory pressure during parsing

### 3.2 Delimiter Bitset Instead of String Contains

**Goal:** Replace O(n) character set membership checks with O(1) bitset lookup.

#### Current (Inefficient)

```rust
if c.is_whitespace() || "()[]{}'`,:@".contains(c) {
    break;
}
```

This calls `String::contains()` which performs character-by-character comparison: O(n) where n=10.

#### Proposed

```rust
const DELIMITER_MASK: u8 = 0b0011_1100;  // ASCII values for delimiters fit in 6 bits

#[inline]
fn is_delimiter(b: u8) -> bool {
    // Check specific delimiters by byte value
    matches!(b, 
        b'(' | b')' | b'[' | b']' | b'{' | b'}' | 
        b'\'' | b'`' | b',' | b':' | b'@')
}

// Or use a static lookup table for maximum speed:
static DELIMITER_TABLE: [bool; 256] = {
    let mut table = [false; 256];
    table[b'(' as usize] = true;
    table[b')' as usize] = true;
    // ... etc
    table
};

#[inline]
fn is_delimiter_fast(b: u8) -> bool {
    DELIMITER_TABLE[b as usize]
}
```

**Benefits:**
- O(1) membership check (inline)
- Compile-time computation via const array
- Branch predictor friendly (simple if-chain vs string search)

### 3.3 Cache List Length in Cons Struct

**Goal:** Avoid O(n) traversal for list length by storing cached length.

#### Current

```rust
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}
```

#### Proposed

```rust
pub struct Cons {
    pub first: Value,
    pub rest: Value,
    pub length: usize,  // Cache for O(1) length lookup
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        let length = match &rest {
            Value::Nil => 1,
            Value::Cons(c) => 1 + c.length,
            _ => 1,  // Improper list
        };
        Cons { first, rest, length }
    }
}
```

#### Usage

```rust
impl Value {
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::Nil => Some(0),
            Value::Cons(c) => Some(c.length),  // O(1) instead of O(n)
            _ => None,
        }
    }
}
```

**Benefits:**
- O(1) length computation
- No traversal, no clones
- Length computed once at construction
- Enables optimizations in primitives

**Tradeoff:** Slightly larger Cons struct (24 bytes → 32 bytes on 64-bit) but massive speedup.

### 3.4 Rc<str> for Symbol Interning

**Goal:** Reduce string allocations in symbol storage from 2 to 1, use reference-counted shared strings.

#### Current

```rust
pub struct SymbolTable {
    map: FxHashMap<String, SymbolId>,  // Owned String keys
    names: Vec<String>,                 // Owned String values
}

pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    let id = SymbolId(self.names.len() as u32);
    self.names.push(name.to_string());      // Allocation 1
    self.map.insert(name.to_string(), id);  // Allocation 2
    id
}
```

#### Proposed

```rust
pub struct SymbolTable {
    map: FxHashMap<Rc<str>, SymbolId>,  // Rc<str> keys (shared)
    names: Vec<Rc<str>>,                 // Rc<str> values (shared)
}

pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    
    let id = SymbolId(self.names.len() as u32);
    let shared_name: Rc<str> = Rc::from(name);  // Single allocation via Rc::from
    self.names.push(shared_name.clone());
    self.map.insert(shared_name, id);
    id
}
```

**Benefits:**
- Single allocation via `Rc::from(name)` leverages string length info
- `Rc<str>` is a single allocation block (no separate data pointer)
- Shared reference counting across both map and vec
- Reduces memory fragmentation

**Measurement:** If app has 10,000 symbols (~500KB total), reduces allocations from 20,000 to 10,000.

### 3.5 Iterator-Based Parsing Instead of Index-Based

**Goal:** Replace manual position tracking with Rust iterators for cleaner, more idiomatic code.

#### Current

```rust
pub struct Reader {
    tokens: Vec<Token>,
    pos: usize,  // Manual tracking
}

impl Reader {
    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.current().cloned();
        self.pos += 1;
        token
    }
}
```

#### Proposed

```rust
pub struct Reader<'a> {
    tokens: &'a [Token],           // Borrow token slice
    iter: std::iter::Peekable<std::slice::Iter<'a, Token>>,
}

impl<'a> Reader<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Reader {
            tokens,
            iter: tokens.iter().peekable(),
        }
    }

    fn current(&mut self) -> Option<&'a Token> {
        self.iter.peek().copied()
    }

    fn advance(&mut self) -> Option<&'a Token> {
        self.iter.next()
    }
}
```

**Benefits:**
- No manual position tracking or bounds checking
- Peekable iterator handles lookahead elegantly
- Borrows instead of copies
- Leverages Rust's zero-cost abstractions
- Compiler can better optimize iterator chains

---

## 4. Detailed Changes by File

### 4.1 src/reader.rs - Tokenizer Rewrite

**Current State:** 647 lines with char-based iteration

**Proposed Changes:**

| Change | Lines | Benefit |
|--------|-------|---------|
| Replace `Vec<char>` with `&[u8]` | 54-60 | No allocation |
| Replace `"()...".contains()` with bitset/table | 167 | O(n) → O(1) |
| Add `StreamingLexer` variant | ~20 | Toggle for testing |
| Refactor symbol reading | 164-174 | Handle UTF-8 properly |
| Update Reader to use Peekable | 357-375 | Iterator-based |

**Size Impact:** +50-75 lines (new bitset table, iterator implementation), -30 lines (simplified logic)

#### Code Example: Delimiter Bitset

**Before:**
```rust
fn read_symbol(&mut self) -> String {
    let mut sym = String::new();
    while let Some(c) = self.current() {
        if c.is_whitespace() || "()[]{}'`,:@".contains(c) {  // O(n)
            break;
        }
        sym.push(c);
        self.advance();
    }
    sym
}
```

**After:**
```rust
const fn is_delimiter(b: u8) -> bool {
    matches!(b,
        b'(' | b')' | b'[' | b']' | b'{' | b'}' |
        b'\'' | b'`' | b',' | b':' | b'@')
}

fn read_symbol(&mut self) -> String {
    let start = self.pos;
    while let Some(b) = self.current() {
        if b.is_ascii_whitespace() || is_delimiter(b) {  // O(1)
            break;
        }
        self.advance_byte();
    }
    // Safe because we validated UTF-8 boundaries
    String::from_utf8_lossy(&self.input[start..self.pos]).into_owned()
}
```

### 4.2 src/value.rs - Add Length Field to Cons

**Current State:** 442 lines

**Proposed Changes:**

| Change | Lines | Benefit |
|--------|-------|---------|
| Add `length: usize` to Cons struct | 52 | O(1) length |
| Update `Cons::new()` constructor | 55-57 | Compute length |
| Add `Value::len()` method | ~8 | Public API |
| Update `list()` helper | 409-413 | Propagate length |
| Update `cons()` helper | 418-420 | Propagate length |

**Size Impact:** +15-20 lines, -10 lines (simplified length computation)

#### Code Example: Length Caching

**Before:**
```rust
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}

// Must traverse entire list:
pub fn list_to_vec(&self) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut current = self.clone();
    loop {
        match current {
            Value::Nil => return Ok(result),
            Value::Cons(cons) => {
                result.push(cons.first.clone());
                current = cons.rest.clone();
            }
            _ => return Err("Not a proper list".to_string()),
        }
    }
}
```

**After:**
```rust
pub struct Cons {
    pub first: Value,
    pub rest: Value,
    pub length: usize,  // Cached length
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        let length = match &rest {
            Value::Nil => 1,
            Value::Cons(c) => 1 + c.length,
            _ => 1,  // Improper list, still counts as 1
        };
        Cons { first, rest, length }
    }
}

// Constant-time:
impl Value {
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::Nil => Some(0),
            Value::Cons(c) => Some(c.length),  // O(1)!
            _ => None,
        }
    }
}
```

### 4.3 src/symbol.rs - Change to Rc<str> Storage

**Current State:** 139 lines

**Proposed Changes:**

| Change | Lines | Benefit |
|--------|-------|---------|
| Replace String with Rc<str> in map | 34 | Shared ownership |
| Replace String with Rc<str> in vec | 35 | Shared ownership |
| Update intern() method | 52-61 | Single allocation |
| Add convenience method for name lookup | ~5 | Borrow Rc<str> |

**Size Impact:** +10-15 lines (documentation/convenience methods)

#### Code Example: Rc<str> Interning

**Before:**
```rust
pub struct SymbolTable {
    map: FxHashMap<String, SymbolId>,
    names: Vec<String>,
}

pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    let id = SymbolId(self.names.len() as u32);
    self.names.push(name.to_string());      // Alloc 1: Vec allocation + data copy
    self.map.insert(name.to_string(), id);  // Alloc 2: HashMap allocation + key copy
    id
}
```

**After:**
```rust
pub struct SymbolTable {
    map: FxHashMap<Rc<str>, SymbolId>,
    names: Vec<Rc<str>>,
}

pub fn intern(&mut self, name: &str) -> SymbolId {
    if let Some(&id) = self.map.get(name) {
        return id;
    }
    let id = SymbolId(self.names.len() as u32);
    let shared = Rc::from(name);  // Single allocation!
    self.names.push(shared.clone());
    self.map.insert(shared, id);
    id
}

pub fn name(&self, id: SymbolId) -> Option<&str> {
    self.names.get(id.0 as usize).map(|s| s.as_ref())
}
```

**Measurement:** For 10,000 unique symbols, saves ~1-2MB of allocator overhead and fragmentation.

### 4.4 src/primitives/list.rs - Use Cached Length

**Current State:** (Assumed based on structure)

**Proposed Changes:**

| Change | Benefit |
|--------|---------|
| Update `length` primitive | O(n) → O(1) |
| Update `nth` bounds checking | Faster bounds check |
| Optimize `take`/`drop` patterns | Avoid full traversal |

#### Code Example: Length Primitive

**Before:**
```rust
fn prim_length(args: &[Value]) -> Result<Value, String> {
    match &args[0] {
        Value::Cons(_) => {
            // Manual traversal
            let vec = args[0].list_to_vec()?;
            Ok(Value::Int(vec.len() as i64))
        }
        _ => Err("length requires a list".to_string()),
    }
}
```

**After:**
```rust
fn prim_length(args: &[Value]) -> Result<Value, String> {
    match args[0].len() {
        Some(len) => Ok(Value::Int(len as i64)),
        None => Err("length requires a list".to_string()),
    }
}
```

---

## 5. Code Examples: Before/After Comparisons

### 5.1 Delimiter Checking

**Before (O(n) string contains):**
```rust
let delimiters = "()[]{}'`,:@";
if delimiters.contains(c) {  // Iterates through 10 chars each time
    break;
}
```

**After (O(1) bitset/match):**
```rust
if matches!(c, 
    b'(' | b')' | b'[' | b']' | b'{' | b'}' | 
    b'\'' | b'`' | b',' | b':' | b'@') {  // Branch predictor, inlined
    break;
}
```

**Cost:** 10 comparisons → 1-2 comparisons (branch prediction).

---

### 5.2 List Construction

**Before (no length cache):**
```rust
let list = cons(
    Value::Int(1),
    cons(Value::Int(2), cons(Value::Int(3), Value::Nil))
);

// Later, when we need length:
let len = count_list(&list);  // O(n) traversal + clones
```

**After (cached length):**
```rust
let list = cons(
    Value::Int(1),
    cons(Value::Int(2), cons(Value::Int(3), Value::Nil))
);

// Later, when we need length:
let len = list.len();  // O(1), no traversal
```

**Benefit:** Removes O(n) cost from common operations like `length`, `take`, `drop`.

---

### 5.3 Symbol Interning

**Before (2 allocations per unique symbol):**
```rust
let mut symbols = SymbolTable::new();

// This allocates twice:
let id1 = symbols.intern("my-symbol");  // Allocations: 1 in Vec, 1 in HashMap

// For cached symbols (still one hash lookup):
let id2 = symbols.intern("my-symbol");  // Hash lookup only, no allocations
```

**After (1 allocation per unique symbol via Rc):**
```rust
let mut symbols = SymbolTable::new();

// This allocates once:
let id1 = symbols.intern("my-symbol");  // Allocation: 1 shared Rc<str>

// For cached symbols:
let id2 = symbols.intern("my-symbol");  // Hash lookup, clone Rc (cheap)
```

**Memory savings:** For 10,000 symbols of average 15 bytes: 150KB savings + reduced fragmentation.

---

### 5.4 Length Calculation

**Before (O(n) traversal with clones):**
```rust
impl Value {
    pub fn list_length(&self) -> Result<usize, String> {
        let mut count = 0;
        let mut current = self.clone();  // Clone entire list!
        loop {
            match current {
                Value::Nil => return Ok(count),
                Value::Cons(cons) => {
                    count += 1;
                    current = cons.rest.clone();  // Clone each step
                }
                _ => return Err("not a list".to_string()),
            }
        }
    }
}

// For a 100-element list: 100 Rc clones + 100 pointer traversals
let len = list.list_length();  // ~10µs for 100 elements
```

**After (O(1) cached):**
```rust
impl Value {
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::Nil => Some(0),
            Value::Cons(c) => Some(c.length),  // Direct field access
            _ => None,
        }
    }
}

// For a 100-element list: direct field read
let len = list.len();  // ~5ns for 100 elements
```

**Speedup:** ~2000x for length calculation on 100-element lists.

---

## 6. Performance Projections

### 6.1 Parsing Performance

#### Current Baseline
- Simple number (42): ~500ns
- List of 5 numbers: ~2.1µs (420ns per element)
- Large list (100): ~5.5µs (55ns per element)

#### Projected Improvement Breakdown

| Optimization | Impact | Reasoning |
|--------------|--------|-----------|
| Eliminate Vec<char> | -15-20% | Reduce allocator overhead, improve cache locality |
| Bitset delimiters | -8-12% | O(1) vs O(n) membership checks, branch prediction |
| Streaming lexer | -5-10% | Better CPU caching, fewer roundtrips |
| **Total Combined** | **-30-40%** | Cumulative effect of all optimizations |

#### Projected Results
- Simple number: 500ns → 300-350ns (-30-40%)
- List of 5: 2.1µs → 1.3-1.5µs (-30-40%)
- Large list (100): **5.5µs → 3.3-4.0µs** (-30-40%)

**Target:** Parse 100-element lists in <4µs (3.3µs median).

### 6.2 Symbol Operations Performance

#### Current Baseline
- New symbol: ~150ns (hash + insert)
- Cached symbol: ~30ns (hash lookup)
- 100 unique symbols: ~15-20µs

#### Projected Improvement

| Operation | Current | Projected | Improvement |
|-----------|---------|-----------|-------------|
| New symbol (alloc) | 150ns | 100ns | -33% |
| Cached lookup | 30ns | 25ns | -17% |
| 100 unique | 15µs | 10µs | -33% |
| Memory per symbol | ~40 bytes | ~32 bytes | -20% |

**Reasoning:**
- Rc<str> combines both allocations into single block
- Single reference counting overhead vs. two allocations
- Compiler can optimize Rc::from() better than double to_string()

### 6.3 List Operations Performance

#### Current Baseline (100-element list)
- Length calculation: ~10µs (traversal + clones)
- Access by index: ~5-8µs (O(n) traversal)
- List copying: ~3-5µs (100 Rc clones)

#### Projected Improvement

| Operation | Current | Projected | Improvement |
|-----------|---------|-----------|-------------|
| Length (100 elems) | ~10µs | ~30ns | -99% |
| Bounds checking | ~10µs | ~50ns | -99% |
| Pattern matching | ~5µs | ~2µs | -60% |
| Memory per list | 16 bytes/cell | 24 bytes/cell | +50% size |

**Reasoning:**
- Cached length = O(1) vs O(n) traversal
- No clones needed for length check
- 8-byte length field < cost of traversal for lists >2 elements
- Break-even at ~2 elements

### 6.4 End-to-End Measurement Target

Using iai-callgrind for deterministic instruction counting:

| Benchmark | Current | Target | Reduction |
|-----------|---------|--------|-----------|
| parse_simple | ~1500 instr | ~1000 instr | -33% |
| parse_list | ~8000 instr | ~5200 instr | -35% |
| parse_nested | ~12000 instr | ~7500 instr | -37% |
| intern_first | ~800 instr | ~550 instr | -31% |
| list_to_vec | ~25000 instr | ~15000 instr | -40% |

**Validation Method:**
```bash
cargo bench --bench iai_benchmarks -- --verbose
```

Results should show measurable reduction in instruction counts via iai-callgrind.

---

## 7. Implementation Roadmap

### Phase 1: Streaming Tokenizer (Bytes, Not Chars)
**Estimated Effort:** 8-12 hours  
**Impact:** 15-20% parsing speedup  
**Risk:** Low (isolated to reader.rs)

**Tasks:**
1. Create `StreamingLexer<'a>` struct with `&[u8]` input
2. Implement byte-level iteration with UTF-8 boundary tracking
3. Refactor `read_symbol`, `read_number`, `read_string` for bytes
4. Add unit tests for UTF-8 edge cases (multi-byte chars)
5. Benchmark and verify -15-20% improvement
6. Keep old `Lexer` as fallback for compatibility

**Files Modified:** `src/reader.rs` (+100 lines, -30 lines net)

**Acceptance Criteria:**
- All existing tests pass
- Large list (100 elements) parses in <5.5µs
- No regression in other operations

---

### Phase 2: Delimiter Bitset Instead of String Contains
**Estimated Effort:** 2-3 hours  
**Impact:** 8-12% parsing speedup  
**Risk:** Very low (leaf function)

**Tasks:**
1. Define `const is_delimiter(b: u8) -> bool` function
2. Replace all `"()...".contains()` calls
3. Test with delimiter-heavy input (deeply nested parens)
4. Benchmark improvement

**Files Modified:** `src/reader.rs` (+5 lines, -10 lines net)

**Acceptance Criteria:**
- Parsing improvement measured with criterion
- iai-callgrind shows -8-12% instructions for delimiter-heavy code

---

### Phase 3: Length Caching in Cons Struct
**Estimated Effort:** 6-8 hours  
**Impact:** 99% improvement for length operations  
**Risk:** Medium (touches core Value type, requires careful API changes)

**Tasks:**
1. Add `length: usize` field to `Cons` struct
2. Update `Cons::new()` to compute length from rest
3. Update `cons()` and `list()` helpers
4. Add `Value::len()` method
5. Update all List primitives to use cached length
6. Update tests to verify length field consistency
7. Verify no ABI breakage for FFI code

**Files Modified:**
- `src/value.rs` (+20 lines)
- `src/primitives/list.rs` (various, -50 lines via simplification)

**Acceptance Criteria:**
- `list_to_vec()` performance unchanged
- `length` primitive returns in constant time
- iai-callgrind shows -99% instructions for length operations
- Memory overhead is acceptable (<5% total)

---

### Phase 4: Symbol Rc<str> Interning
**Estimated Effort:** 4-6 hours  
**Impact:** 15-20% symbol memory reduction  
**Risk:** Low-Medium (isolated to symbol.rs, but affects string API)

**Tasks:**
1. Change `SymbolTable::map` from `FxHashMap<String, SymbolId>` to `FxHashMap<Rc<str>, SymbolId>`
2. Change `SymbolTable::names` from `Vec<String>` to `Vec<Rc<str>>`
3. Update `intern()` method to use `Rc::from()`
4. Update `name()` method to return `&str` from `Rc<str>`
5. Verify no allocations increase elsewhere
6. Benchmark symbol table operations
7. Test with 10K+ symbol workload

**Files Modified:** `src/symbol.rs` (+15 lines)

**Acceptance Criteria:**
- Symbol intern time unchanged or better
- Memory usage for symbols reduced by 15-20%
- No regression in symbol lookup performance

---

### Phase 5: Iterator-Based Parsing (Peekable)
**Estimated Effort:** 4-5 hours  
**Impact:** 5-8% overall parsing speedup  
**Risk:** Low (cosmetic improvement, well-supported by Rust stdlib)

**Tasks:**
1. Refactor `Reader` to use `Peekable<Iter<Token>>`
2. Replace `current()` with `peek()`
3. Replace `advance()` with `next()`
4. Simplify position tracking code
5. Verify all error messages still work
6. Benchmark to confirm improvement

**Files Modified:** `src/reader.rs` (-40 lines via simplification)

**Acceptance Criteria:**
- All tests pass
- Code is more idiomatic Rust
- Benchmarks show -5-8% improvement

---

### Timeline Summary

| Phase | Hours | Cumulative | Cumulative Speedup |
|-------|-------|-----------|-------------------|
| 1: Streaming | 10 | 10 | 15-20% |
| 2: Bitset | 3 | 13 | 23-30% |
| 3: Length Cache | 7 | 20 | 23-30% (list-focused) |
| 4: Rc<str> | 5 | 25 | 23-30% (symbol-focused) |
| 5: Iterators | 4 | 29 | 28-38% |

**Total Effort:** ~29 hours over 2-3 sprints
**Total Expected Improvement:** 28-40% for parsing, up to 99% for specific operations

---

## 8. Risks & Mitigations

### 8.1 Breaking Changes to Public API

**Risk:** Medium  
**Impact:** Downstream code using internal structures

**Changes That Break API:**
- `Lexer::new()` signature doesn't change, but internal `Vec<char>` → `&[u8]` is invisible
- `Cons` struct gains a field (length) - binary compatible via repr
- `SymbolTable` uses `Rc<str>` instead of `String` - API compatible, internal change

**Mitigation:**
- Ensure all public APIs remain at same signatures
- Internal struct changes are fine (Cons is private to Value enum)
- SymbolTable changes are backwards-compatible (methods unchanged)
- Test with existing examples to verify no breaking changes

---

### 8.2 Testing Strategy

**Unit Tests:**
- UTF-8 boundary cases in streaming lexer (multi-byte chars)
- Delimiter detection (all special chars)
- Length field consistency after list operations
- Symbol deduplication (intern multiple times, verify same ID)

**Integration Tests:**
- Benchmark suite must pass with improvement targets
- Parsing all existing test programs
- REPL functionality unchanged
- FFI code still works

**Regression Tests:**
- No performance degradation in other phases (compiler, VM)
- No memory usage increase overall
- No new allocations in hot paths

---

### 8.3 Benchmarking Approach

**Criterion Benchmarks** (benches/benchmarks.rs):
- Measure wall-clock time for each parsing scenario
- Run with multiple sample sizes
- Generate HTML reports comparing before/after

**IAI-Callgrind Benchmarks** (benches/iai_benchmarks.rs):
- Deterministic instruction counting (no noise)
- No randomness, run once
- Compare instruction counts for key operations
- Useful for inlining/optimization decisions

**Custom Benchmarks:**
- Parse real-world Elle programs from examples/
- Measure end-to-end time (parse + compile + execute)
- Identify any new bottlenecks

---

### 8.4 Backward Compatibility

**What Must Not Change:**
- Public function signatures (read_str, intern, cons)
- Token enum values
- Value enum variants
- SymbolId behavior (still a u32 wrapper)

**What Can Change (Internal):**
- Lexer struct fields (Vec<char> → &[u8])
- Reader struct (can use iterators instead of pos)
- SymbolTable internal storage (String → Rc<str>)
- Cons struct (can add length field)

**Verification:**
- Compile existing examples without changes
- Run existing test suite unchanged
- Check binary serialization (if any) for compatibility

---

## 9. Acceptance Criteria

### 9.1 Performance Targets

- [ ] **Parsing benchmark < 4µs** for 100-element list (5.5µs baseline)
  - Measured via `criterion` with -95% confidence interval
  - Tested on release build with optimizations
  
- [ ] **Symbol interning < 100ns** for new symbol
  - 150ns baseline, target -33%
  - Verified with iai-callgrind

- [ ] **List length operation O(1)** (was O(n))
  - Measured via iai-callgrind showing constant instructions
  - No variance with list size

### 9.2 Quality Targets

- [ ] **All existing tests pass** without modification
  - `cargo test --release` succeeds
  - No warnings or errors
  
- [ ] **No regressions** in other operations
  - Compilation speed unchanged or better
  - VM execution speed unchanged or better
  - Memory usage unchanged or slightly better
  
- [ ] **Instruction count improvements measurable**
  - iai-callgrind shows -25-35% reduction for parsing
  - iai-callgrind shows -99% reduction for length operations
  - Report generated with baseline vs. new comparison

### 9.3 Code Quality Targets

- [ ] **Zero unsafe code** added (except necessary FFI)
- [ ] **Clear documentation** of optimizations
- [ ] **No clippy warnings** in modified code
- [ ] **Comprehensive test coverage** for new features

---

## 10. Implementation Checklist

### Pre-Implementation
- [ ] Establish baseline benchmarks (run current suite)
- [ ] Document current performance characteristics
- [ ] Set up profiling environment (iai-callgrind, criterion)

### Phase 1: Streaming Tokenizer
- [ ] Create StreamingLexer<'a> with &[u8]
- [ ] Implement byte-level iteration
- [ ] Handle UTF-8 boundaries in symbol reading
- [ ] Write comprehensive unit tests
- [ ] Benchmark and verify -15-20% improvement
- [ ] Code review and documentation

### Phase 2: Bitset Delimiters
- [ ] Define is_delimiter() function
- [ ] Replace all contains() calls
- [ ] Test with delimiter-heavy input
- [ ] Benchmark improvement
- [ ] Code review

### Phase 3: Length Caching
- [ ] Add length field to Cons struct
- [ ] Update Cons::new() constructor
- [ ] Add Value::len() method
- [ ] Update List primitives
- [ ] Test for consistency
- [ ] Benchmark list operations
- [ ] Code review

### Phase 4: Rc<str> Interning
- [ ] Update SymbolTable to use Rc<str>
- [ ] Test symbol deduplication
- [ ] Verify memory usage improvement
- [ ] Benchmark symbol operations
- [ ] Code review

### Phase 5: Iterator-Based Parsing
- [ ] Refactor Reader for Peekable<Iter>
- [ ] Simplify position tracking
- [ ] Test all parsing scenarios
- [ ] Benchmark overall improvement
- [ ] Code review

### Post-Implementation
- [ ] Run full test suite
- [ ] Verify all benchmarks meet targets
- [ ] Generate performance report
- [ ] Update architecture documentation
- [ ] Create pull request with all changes

---

## 11. Appendix: Reference Implementation Sketches

### A. StreamingLexer Implementation Sketch

```rust
pub struct StreamingLexer<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> StreamingLexer<'a> {
    pub fn new(input: &'a str) -> Self {
        StreamingLexer {
            input: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    #[inline]
    fn current_byte(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance_byte(&mut self) {
        if let Some(b) = self.current_byte() {
            if b == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.pos += 1;
        }
    }

    fn read_symbol(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.current_byte() {
            if b.is_ascii_whitespace() || self.is_delimiter_byte(b) {
                break;
            }
            self.advance_byte();
        }
        // String::from_utf8_lossy handles edge cases
        String::from_utf8_lossy(&self.input[start..self.pos]).into_owned()
    }

    const fn is_delimiter_byte(&self, b: u8) -> bool {
        matches!(b,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' |
            b'\'' | b'`' | b',' | b':' | b'@')
    }
}
```

### B. Cons with Length Cache

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Cons {
    pub first: Value,
    pub rest: Value,
    pub length: usize,  // New field
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        let length = match &rest {
            Value::Nil => 1,
            Value::Cons(c) => 1 + c.length,
            _ => 1,  // Improper list
        };
        Cons { first, rest, length }
    }
}

impl Value {
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::Nil => Some(0),
            Value::Cons(c) => Some(c.length),
            _ => None,
        }
    }
}
```

### C. Rc<str> Symbol Interning

```rust
pub struct SymbolTable {
    map: FxHashMap<Rc<str>, SymbolId>,
    names: Vec<Rc<str>>,
}

impl SymbolTable {
    pub fn intern(&mut self, name: &str) -> SymbolId {
        if let Some(&id) = self.map.get(name) {
            return id;
        }
        
        let id = SymbolId(self.names.len() as u32);
        let shared = Rc::from(name);
        self.names.push(shared.clone());
        self.map.insert(shared, id);
        id
    }

    pub fn name(&self, id: SymbolId) -> Option<&str> {
        self.names.get(id.0 as usize).map(|s| s.as_ref())
    }
}
```

---

## 12. Document Control

**Version History:**
| Version | Date | Author | Notes |
|---------|------|--------|-------|
| 1.0 | 2026-02-07 | Technical Team | Initial design document |

**Review Checklist:**
- [ ] Technical accuracy verified
- [ ] Performance projections justified
- [ ] Risk assessment complete
- [ ] Implementation plan is realistic
- [ ] Acceptance criteria are measurable

---

**End of Document**
