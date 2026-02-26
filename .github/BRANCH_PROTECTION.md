# Branch Protection Rules for Elle

This document describes the branch protection configuration for the Elle repository.

## Protected Branches

The following branches are protected and cannot be force-pushed or deleted:
- `main` - Production release branch
- `develop` - Development integration branch

## Protection Rules for Main Branch

### Require Status Checks to Pass Before Merging

All CI/CD checks must pass before a PR can be merged:

1. **Test Suite** (`test`) - Unit, integration, and doc tests on stable, beta, and nightly Rust
2. **Rustfmt** (`fmt`) - Code formatting compliance
3. **Clippy** (`clippy`) - Linting and code quality analysis
4. **Dependency Audit** (`audit`) - Security vulnerability scanning
5. **Examples** (`examples`) - Example compilation and verification
6. **Benchmarks** (`benchmarks`) - Performance regression detection
7. **Documentation** (`docs`) - Documentation generation and build validation

### All-Checks Status

An additional check (`all-checks`) waits for all above checks to complete and reports the final status. This is the check that must pass.

> **Note**: Validation checks (test, fmt, clippy, examples, benchmarks) run
> on pull request events only. When a PR is merged to main, the push event
> triggers coverage, benchmark publishing, documentation generation, and
> Pages deployment — the validation checks are not re-run since they already
> passed on the PR.

### Additional Settings

- **Require branches to be up to date before merging** - PRs must be rebased with main before merge
- **Require code reviews before merging** - At least 1 approval required (or owner review if user-owned)
- **Require status checks from a branch protection rule** - Configured above
- **Allow force pushes** - Disabled (no one can force push)
- **Allow deletions** - Disabled (no one can delete the branch)

## How to Enforce These Rules

Run the following GitHub CLI command:

```bash
# Set up main branch protection
gh api repos/disruptek/elle/branches/main/protection \
  -f required_status_checks='{
    "strict": true,
    "contexts": ["All Checks Passed"]
  }' \
  -f enforce_admins=true \
  -f required_pull_request_reviews='{
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": false,
    "required_approving_review_count": 1
  }' \
  -f restrictions=null

# Or use the GitHub Web UI:
# 1. Navigate to Settings → Branches
# 2. Click "Add rule" for "main"
# 3. Configure as described above
```

## What This Means for Contributors

### For Feature Development

1. Create a feature branch from `develop`:
   ```bash
   git checkout develop
   git pull origin develop
   git checkout -b feature/my-feature
   ```

2. Make commits following conventional commits:
   ```bash
   git commit -m "feat: add new feature"
   git commit -m "fix: resolve issue"
   git commit -m "docs: update documentation"
   ```

3. Push your branch and create a pull request:
   ```bash
   git push origin feature/my-feature
   ```

4. The CI pipeline will automatically run all checks

5. Once all checks pass and you have at least 1 approval, you can merge with squash strategy

### For Bug Fixes

1. Create a fix branch:
   ```bash
   git checkout develop
   git checkout -b fix/issue-description
   ```

2. Commit and push:
   ```bash
   git commit -m "fix: issue description"
   git push origin fix/issue-description
   ```

3. Open PR against `develop`

4. Once checks pass and approved, merge

### Merging to Main

PRs to `main` should only come from `develop` and must:
- Have all status checks passing
- Be reviewed and approved
- Use squash merge strategy (see MERGE_STRATEGY.md)

## Important Notes

- **No force pushes**: Maintain a clean history
- **No direct commits to main**: Always use PR workflow
- **Status checks required**: Cannot bypass these checks
- **Code reviews required**: At least one approval needed
- **Up-to-date requirement**: Must rebase with latest main before merging

## Maintenance

If status checks need to be updated:

1. Modify `.github/workflows/ci.yml`
2. Update the `all-checks` job to include new checks
3. Test in develop branch first
4. Merge to main via PR
5. Update this documentation if new checks are added

---

**Last Updated**: February 26, 2026
