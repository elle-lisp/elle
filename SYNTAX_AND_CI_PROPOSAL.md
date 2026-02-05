# Elle Language: Syntax Evolution & CI Enhancement Proposal

Combined, these changes position Elle as a modern Lisp suitable for production use while maintaining its elegant simplicity.

---

## Part 1: CI/CD Improvements

### Current Strengths ✅
- Comprehensive testing (unit, integration, doc, benchmark)
- Multi-version testing (stable, beta, nightly)
- Code quality enforcement (clippy -D warnings, rustfmt)
- Coverage tracking (Codecov)
- Documentation generation

### Proposed Additions

#### 🔒 Security Layer
1. **Dependency Audit** (`cargo audit`)
   - Scan for known vulnerabilities
   - Block builds on critical issues
   - Estimated effort: 30 minutes

2. **SBOM Generation** (Software Bill of Materials)
   - Transparency for users
   - Supply chain security
   - Automated reporting
   - Estimated effort: 1-2 hours

3. **SLSA Provenance** (Build integrity)
   - Verify build artifacts
   - Supply chain protection
   - Compliance ready
   - Estimated effort: 2-3 hours

#### 📊 Performance Tracking
1. **Regression Detection**
   - Alert on >10% slowdown
   - Benchmark trending
   - Comment on PRs with results
   - Estimated effort: 2-3 hours

2. **Flamegraph Benchmarks**
   - Visual performance analysis
   - Artifact storage
   - Identify bottlenecks
   - Estimated effort: 1-2 hours

#### 🧪 Correctness Verification
1. **Miri Testing** (UB Detection)
   - Catch undefined behavior in unsafe code
   - Nightly testing (optional failure)
   - Estimated effort: 30 minutes

2. **Semantic Commits**
   - Enforce conventional commits
   - Auto-changelog generation
   - Better git history
   - Estimated effort: 1-2 hours

#### 🚀 Release Automation
1. **Semantic Versioning**
   - Auto-bump version
   - Generate changelogs
   - Tag releases
   - Estimated effort: 2-3 hours

### Implementation Phases

| Phase | Timeline | Items | Risk |
|-------|----------|-------|------|
| Phase 1 | Day 1 | Audit, Miri, Semantic Commits | Low |
| Phase 2 | Week 1 | SBOM, Performance Detection | Medium |
| Phase 3 | Week 2 | SLSA Provenance, Flamegraphs | Medium |
| Phase 4 | Week 3 | Semantic Release | Low |

### Total Estimated Effort: 12-16 hours

---

## Part 2: Syntax Evolution

### Language Comparison

| Feature | Scheme | Janet | Clojure | Fennel | Elle (Proposed) |
|---------|--------|-------|---------|--------|-----------------|
| Vector literal | `(vector 1 2)` | `@[1 2]` | `[1 2]` | `[1 2]` | `[1 2]` ✨ |
| Map literal | ❌ | `{:k v}` | `{:k v}` | `{:k v}` | `{:k v}` ✨ |
| Keywords | Symbols | Symbols | Type | Symbols | Type ✨ |
| Destructuring | ❌ | ✅ | ✅ | ✅ | ✅ ✨ |
| Threading | ❌ | ❌ | `-> ->>` | ❌ | `-> ->>` ✨ |
| Varargs | ✅ | ✅ | ✅ | ✅ | `& args` ✨ |
| Implicit returns | ❌ | ✅ | ✅ | ✅ | ✅ ✨ |

### Phase 1: Data Literals (2-3 weeks)

**Goal**: Make common data structures obvious and concise

```lisp
; BEFORE
(define data (vector 1 2 3))
(define config (list :x 1 :y 2))

; AFTER
(define data [1 2 3])
(define config {:x 1 :y 2})
```

**Implementation**:
- Lexer: Add bracket `[]` and brace `{}` tokens
- Reader: `[...]` → `(vector ...)`, `{:k v}` → `(map (list :k v))`
- No compiler changes needed
- Risk: Low (purely syntactic sugar)

### Phase 2: Keywords as First-Class (3-4 weeks)

**Goal**: Distinguish data from code, enable keyword invocation

```lisp
; Keywords are now a type, not symbols
(define kw :name)
(keyword? kw)  ; => true

; Keywords can access maps
(:name {:name "Alice"})  ; => "Alice"
```

**Implementation**:
- Type system: Add `Keyword` variant
- Compiler: Optimize `(keyword map)` to `(get map keyword)`
- No runtime overhead
- Risk: Medium (type system changes)

### Phase 3: Destructuring (4-6 weeks)

**Goal**: Eliminate boilerplate for unpacking structures

```lisp
; Vector destructuring
(let [x y] [1 2]
  (+ x y))

; Map destructuring
(let {:name name :age age} person
  (printf "%s is %d\n" name age))

; Nested destructuring
(let [[a [b c]] data]
  (list c b a))
```

**Implementation**:
- Compiler: Detect pattern syntax in let/fn bindings
- Generate indexed access or get calls
- No runtime overhead
- Risk: High (complex compiler changes)

### Phase 4: Threading Macros (2 weeks)

**Goal**: Improve readability of nested function calls

```lisp
; Thread-first: pass result as first arg
(-> "  hello  "
    string-trim
    string-upcase
    display)

; Thread-last: pass result as last arg
(->> [1 2 3 4]
     (map inc)
     (filter even?)
     (apply +))
```

**Implementation**:
- Macros: Simple transformations
- No compiler changes needed
- Risk: Low (pure macros)

### Phase 5: Function Definition Sugar (1-2 weeks)

**Goal**: More intuitive function definitions

```lisp
; Shorthand lambda
(define inc (fn [x] (+ x 1)))

; Top-level function definition
(defn add [x y]
  (+ x y))

; With docstring and defaults
(defn greet [name "World"]
  (printf "Hello %s!\n" name))
```

**Implementation**:
- Macros for `fn` and `defn`
- Optional defaults via compiler support
- Risk: Low (macros + simple extensions)

### Phase 6: Advanced Features (6+ weeks, optional)

- Multi-arity dispatch
- Pattern matching
- Keyword arguments
- Set comprehensions

---

## Comparison: Before & After

### Example 1: Configuration Data
```lisp
; BEFORE (Scheme-style)
(define config
  (list :host "localhost" :port 8080 :ssl #f))
(define host (first config))

; AFTER (Janet/Clojure-style)
(define config {:host "localhost" :port 8080 :ssl #f})
(define host (:host config))
```
**Reduction**: 20% fewer characters, clearer intent

### Example 2: Data Processing
```lisp
; BEFORE (Scheme-style)
(define process
  (lambda (data)
    (let ((filtered (filter even? data)))
      (map inc filtered))))

; AFTER (Modern Lisp-style)
(defn process [data]
  (->> data
       (filter even?)
       (map inc)))
```
**Reduction**: 30% fewer lines, left-to-right flow

### Example 3: Destructuring
```lisp
; BEFORE (Scheme-style)
(define parse-point
  (lambda (point)
    (let ((x (first point))
          (y (first (rest point))))
      (+ x y))))

; AFTER (Modern Lisp-style)
(defn parse-point [[x y]]
  (+ x y))
```
**Reduction**: 50% fewer lines, intent obvious

---

## Implementation Strategy

### Parser Architecture
```
Lexer         → Tokens: +, [, {, :name, etc.
    ↓
Reader        → S-expressions + literals
    ↓
Compiler      → Bytecode (with optimizations)
    ↓
VM            → Execution
```

### Backward Compatibility
- **No breaking changes**: Old syntax still works
- **Gradual adoption**: Users migrate at their pace
- **Linter support**: Suggest modern syntax
- **Documentation**: Show both syntaxes during transition

### Testing Strategy
```
✅ Lexer tests       - New token recognition
✅ Reader tests      - Literal conversion
✅ Compiler tests    - Transformations
✅ Integration tests - Multi-phase combinations
✅ Compat tests      - Old syntax still works
```

---

## Timeline & Effort Estimates

### CI/CD Improvements
- **Phase 1** (2 days): Audit, Miri, Semantic Commits
- **Phase 2** (3 days): SBOM, Performance Detection
- **Phase 3** (3 days): SLSA, Flamegraphs
- **Phase 4** (2 days): Release Automation
- **Total**: ~10 days or 80 hours

### Syntax Improvements
- **Phase 1** (2 weeks): Vectors, Maps, Sets
- **Phase 2** (3-4 weeks): Keywords as Type
- **Phase 3** (4-6 weeks): Destructuring
- **Phase 4** (2 weeks): Threading
- **Phase 5** (1-2 weeks): Function Sugar
- **Total**: ~18-24 weeks or 600+ hours (can be parallelized)

---

## Risks & Mitigation

### CI/CD Risks
| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|-----------|
| Build slowdown | Medium | Medium | Run expensive checks on nightly |
| False positives | Low | Low | Use soft-fail for experimental checks |
| Increased complexity | Low | Low | Document all new workflows |

### Syntax Risks
| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|-----------|
| Grammar conflicts | Low | High | Careful lexer design, early testing |
| Backward compat | Low | High | Keep old syntax, extensive tests |
| Learning curve | Medium | Low | Good docs, gradual rollout |
| Compiler complexity | Medium | Medium | Incremental phases, PRs |

---

## Success Metrics

### CI/CD
- ✅ Zero security vulnerabilities in releases
- ✅ <2% performance regression per release
- ✅ 100% build provenance coverage
- ✅ Automated releases without manual steps

### Syntax
- ✅ 30-40% reduction in boilerplate for data code
- ✅ <5% of test suite breaking with new syntax
- ✅ All old syntax still compiles
- ✅ Community feedback positive on usability

---

## Recommendations

### Immediate (This Sprint)
1. **Add Audit & Miri** (low hanging fruit)
2. **Start Phase 1 Syntax** (vectors & maps)
3. **Semantic Commits** (governance)

### Near-term (Next 2 Sprints)
1. **Complete Phase 1** (data literals)
2. **Add Performance Detection**
3. **Phase 2 Keywords** (type system)

### Medium-term (Following Month)
1. **Phase 3 Destructuring** (complex, but high impact)
2. **Phase 4 Threading**
3. **Release Automation**

### Long-term (As Capacity Allows)
1. **Phase 5 Function Sugar**
2. **Phase 6 Advanced Features**

---

## References

- **Janet**: https://janet-lang.org/ (practical concessions)
- **Clojure**: https://clojure.org/ (rich literals, destructuring)
- **Fennel**: https://fennel-lang.org/ (Lua-friendly syntax)
- **SLSA Framework**: https://slsa.dev/
- **Semantic Versioning**: https://semver.org/

