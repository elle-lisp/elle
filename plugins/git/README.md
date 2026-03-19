# elle-git

Git repository access for Elle via the `git2` crate.

## Building

```bash
cargo build -p elle-git --release
# Output: target/release/libelle_git.so
```

## Usage

```lisp
(import-file "target/release/libelle_git.so")
(let ((repo (git/open "/path/to/repo")))
  (git/status repo))
```

## Primitives

### Repository

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/open` | `path` | git/repo | Open existing repository |
| `git/init` | `path` | git/repo | Initialize new repository |
| `git/clone` | `url path` | git/repo | Clone from URL |
| `git/path` | `repo` | string | Path to `.git` directory |
| `git/workdir` | `repo` | string or nil | Working directory path |
| `git/bare?` | `repo` | bool | True if bare repository |
| `git/state` | `repo` | keyword | Repository state |

### HEAD and References

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/head` | `repo` | struct | HEAD info: `{:name :oid :symbolic}` |
| `git/resolve` | `repo ref` | string | Resolve ref to OID hex |

### Branches

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/branches` | `repo [filter]` | list | List branches |
| `git/branch-create` | `repo name [target]` | string | Create branch, returns OID |
| `git/branch-delete` | `repo name` | nil | Delete local branch |
| `git/checkout` | `repo ref [opts]` | nil | Checkout ref |

### Commits

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/commit-info` | `repo oid` | struct | Read commit by OID |
| `git/log` | `repo [opts]` | list | Walk commit history |
| `git/commit` | `repo message [opts]` | string | Create commit, returns OID |

### Index (Staging)

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/add` | `repo path-or-paths` | nil | Stage file(s) |
| `git/remove` | `repo path-or-paths` | nil | Unstage file(s) |
| `git/add-all` | `repo` | nil | Stage all changes |

### Status

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/status` | `repo` | list | List file statuses |

### Diff

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/diff` | `repo [opts]` | struct | Diff summary with file/hunk/line detail |
| `git/diff-patch` | `repo [opts]` | string | Full unified diff as text |

### File Contents

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/show` | `repo ref path` | string | File contents at a ref |

### Tags

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/tags` | `repo` | list | List tag names |
| `git/tag-create` | `repo name [target] [message]` | string | Create tag |
| `git/tag-delete` | `repo name` | nil | Delete tag |

### Remotes

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/remotes` | `repo` | list | List remote names |
| `git/remote-info` | `repo name` | struct | Remote details |
| `git/fetch` | `repo remote-name` | nil | Fetch from remote |
| `git/push` | `repo remote-name [refspecs]` | nil | Push to remote |

### Config

| Primitive | Args | Returns | Description |
|-----------|------|---------|-------------|
| `git/config-get` | `repo key` | string or nil | Read config value |
| `git/config-set` | `repo key value` | nil | Write config value |

## Error Handling

All git errors use kind `"git-error"`. Type errors use `"type-error"`.

```lisp
(protect (git/open "/nonexistent"))
;; => [false {:kind :git-error :message "git/open: ..."}]
```

## Authentication

v1 supports public repos, SSH agent, and system credential helpers.
Username/password prompts and custom key paths are deferred to v2.
