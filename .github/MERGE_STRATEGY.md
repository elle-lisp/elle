# Merge Strategy for Elle Repository

This document describes the merge strategy and best practices for contributing to Elle.

## Overview

Elle uses a **squash merge strategy** to maintain a clean, linear commit history on the main branch while preserving detailed commit history on feature branches.

## Merge Strategies Explained

### 1. Squash Merge (Recommended) ✅

**What it does**: Combines all commits from the PR into a single commit on main

**Command**:
```bash
git merge --squash feature/my-feature
git commit -m "feat: descriptive commit message"
```

**GitHub UI**: Select "Squash and merge" option

**Pros**:
- Clean, linear history on main
- Easy to revert single features
- Bisect becomes more meaningful
- Release notes are clear

**Cons**:
- Loses individual commit messages (but PR description is preserved)

**Use When**: This is the standard merge strategy for Elle

### 2. Create a Merge Commit

**What it does**: Creates a merge commit that links PR to main

**Command**:
```bash
git merge --no-ff feature/my-feature
```

**GitHub UI**: Select "Create a merge commit" option

**Pros**:
- Preserves all commit history
- Clear audit trail

**Cons**:
- Can clutter main branch history
- Harder to bisect

**Use When**: Only for large feature sets (Phase implementations, major refactors)

### 3. Rebase and Merge

**What it does**: Replays commits from PR onto main without a merge commit

**Command**:
```bash
git rebase main feature/my-feature
git merge --ff-only feature/my-feature
```

**GitHub UI**: Select "Rebase and merge" option

**Pros**:
- Linear history similar to squash
- Preserves individual commits
- Clean cherry-pick history

**Cons**:
- Forces branch rewrite (may confuse contributors)

**Use When**: Special cases where individual commits must be preserved

---

## Standard Workflow

### For Regular PRs (Bug Fixes, Features, Documentation)

1. **Create feature branch**:
   ```bash
   git checkout develop
   git pull origin develop
   git checkout -b feature/descriptive-name
   ```

2. **Make commits** with clear, conventional messages:
   ```bash
   git commit -m "feat: add new capability"
   git commit -m "fix: resolve edge case"
   git commit -m "docs: update guide"
   ```

3. **Push and open PR**:
   ```bash
   git push origin feature/descriptive-name
   # Open PR against develop branch
   ```

4. **Wait for CI checks** - All status checks must pass

5. **Request review** - At least 1 approval required

6. **Merge using squash**:
   - On GitHub: Click "Squash and merge"
   - Write squash commit message summarizing the changes
   - Confirm merge

7. **Delete branch** - GitHub offers to delete after merge

### For Release PRs (Main Branch)

Only project maintainers merge to main. Process:

1. **Create release branch from develop**:
   ```bash
   git checkout develop
   git pull origin develop
   git checkout -b release/v1.0.0
   ```

2. **Update version numbers** and CHANGELOG

3. **Create PR to main branch**

4. **All checks must pass**

5. **Squash merge** with commit message:
   ```
   chore: release v1.0.0
   
   - Feature 1
   - Feature 2
   - Bug fix 1
   ```

6. **Tag the release**:
   ```bash
   git tag -a v1.0.0 -m "Release v1.0.0"
   git push origin v1.0.0
   ```

---

## Conventional Commits

To maintain a readable history, follow conventional commits:

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, missing semicolons, etc.)
- `refactor`: Code refactoring without feature changes
- `perf`: Performance improvements
- `test`: Test additions or modifications
- `chore`: Build process, CI/CD, dependency updates
- `ci`: CI/CD configuration changes

### Examples

```bash
# Feature with scope
git commit -m "feat(parser): add support for quasiquote syntax"

# Bug fix with scope
git commit -m "fix(vm): resolve stack overflow in recursive calls"

# Documentation
git commit -m "docs: add FFI binding guide"

# CI/CD changes
git commit -m "ci: add dependency audit to pipeline"

# With body for more detail
git commit -m "refactor(compiler): simplify bytecode generation

- Split compilation into passes
- Add optimization pass
- Reduce code duplication

This improves maintainability and enables future optimizations."
```

---

## Squash Commit Message Guidelines

When squashing PRs, the commit message should:

1. **Start with conventional commit type**: `feat`, `fix`, `docs`, etc.
2. **Include scope if applicable**: `feat(parser)`, `fix(vm)`
3. **Summarize the entire PR**: What does this PR accomplish?
4. **Reference the PR number**: Closes #123
5. **Be concise but descriptive**: 50 characters max for subject line

### Examples

```
feat(parser): add pattern matching with guards

Implements pattern matching with support for:
- Literal patterns
- Wildcard patterns (_)
- Guard expressions

Closes #156
```

```
fix(ffi): resolve type marshaling for struct pointers

Previously, struct pointers weren't correctly unmarshaled
back to Elle values. Now properly handles opaque pointers
and GC integration.

Closes #142
```

```
docs: consolidate roadmap files into single document

Merges FFI_ROADMAP.md and UNIMPLEMENTED_FEATURES.md into
comprehensive ROADMAP.md for easier maintenance and reference.
```

---

## Branch Naming Conventions

Use descriptive branch names following this pattern:

```
<type>/<description>
```

**Types**:
- `feature/` - New feature
- `fix/` - Bug fix
- `docs/` - Documentation
- `refactor/` - Code refactoring
- `test/` - Test additions
- `chore/` - Maintenance tasks
- `ci/` - CI/CD changes

**Examples**:
```
feature/pattern-matching-guards
fix/closure-scope-bug
docs/ffi-binding-guide
refactor/compiler-phases
test/add-edge-case-coverage
chore/update-dependencies
ci/add-dependency-audit
```

---

## Best Practices

### Before Merging

- ✅ All CI checks pass
- ✅ At least 1 code review approval
- ✅ Branch is up to date with base branch
- ✅ No conflicts
- ✅ Commit messages are clear

### During Merge

- ✅ Write a meaningful squash commit message
- ✅ Reference PR number if closing an issue
- ✅ Use conventional commit format
- ✅ Keep message to ~50 characters for subject line

### After Merge

- ✅ Delete the feature branch
- ✅ Monitor CI/CD of main branch
- ✅ Create release notes if needed

---

## Handling Merge Conflicts

If your PR has conflicts with the base branch:

```bash
# Update your branch with latest base branch
git fetch origin
git rebase origin/develop

# Resolve conflicts in your editor
# Then continue rebase
git rebase --continue

# Force push to update PR
git push -f origin feature/my-feature
```

Or use the GitHub UI's "Update branch" button to rebase.

---

## Reverting Changes

If a merged commit needs to be reverted:

```bash
# Identify the commit to revert
git log --oneline main | head -20

# Create a revert commit
git revert <commit-hash>

# This creates a new commit that undoes the changes
git push origin main
```

The revert commit shows what was removed and when, maintaining history.

---

## Release Management

### Version Numbering

Elle uses Semantic Versioning: `MAJOR.MINOR.PATCH`

- `MAJOR`: Breaking changes
- `MINOR`: New features (non-breaking)
- `PATCH`: Bug fixes (non-breaking)

### Release Process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create `release/vX.Y.Z` branch
4. Create PR to `main`
5. After merge, tag: `git tag -a vX.Y.Z`
6. Push tag: `git push origin vX.Y.Z`
7. Publish to crates.io: `cargo publish`

---

## FAQ

**Q: Why squash merges?**  
A: Keeps main branch clean with one commit per PR, easy to revert, clear release notes.

**Q: What if I need to preserve commit history?**  
A: Use "Create a merge commit" for Phase-level implementations or other cases where history matters.

**Q: Can I force push?**  
A: No, force pushes are disabled on protected branches. Create a new commit instead.

**Q: How do I update my PR with main changes?**  
A: Use GitHub's "Update branch" button or `git rebase origin/main` locally.

**Q: What if my commit message is wrong?**  
A: You can change it when squashing the PR. GitHub will show the option.

---

**Last Updated**: February 5, 2026
