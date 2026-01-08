# Weaver

Issue tracking designed for AI coding agents.

Weaver stores issues as markdown files with YAML frontmatter, making it easy for both humans and AI agents to read and modify issues. It features dependency tracking with cycle detection, automatic "ready queue" computation, workflows, hints, and autonomous agent launch.

## Quick Reference

| Task | Command |
|------|---------|
| Find ready work | `weaver ready` |
| View with context | `weaver show <id> --fetch-deps` |
| Start work | `weaver start <id>` |
| Create issue | `weaver create "Title" -t task -p 1` |
| Complete work | `weaver close <id>` |
| Search project knowledge | `weaver hint search <query>` |
| Launch autonomous agent | `weaver launch <id> --model sonnet` |

## Installation

```bash
uvx git+https://github.com/rjpower/weaver

or 

uv pip install -e .

for local development
```

## Core Concepts

**Issues**: Markdown files with YAML frontmatter in `.weaver/issues/`. Human and agent readable.

**Dependencies**: Issues can block other issues. Weaver detects cycles and computes the ready queue automatically. Use `weaver dep add <child> <parent>` to mark dependencies.

**Ready Queue**: `weaver ready` shows unblocked issues sorted by priority. Use this to find your next task.

**Hints**: Persistent project knowledge stored in `.weaver/hints/`. Use for coding conventions, architecture notes, or domain knowledge that agents need across sessions. Searchable with `weaver hint search`.

**Workflows**: YAML templates in `.weaver/workflows/` that create multiple linked issues at once. Useful for repeatable processes like "add new API endpoint" or "bug triage."

**Launch**: Spawn autonomous Claude agents to work on issues with full context injection. Logs stored in `.weaver/launches/`.

**Sync**: Push/pull issues to a git branch for sharing across machines or team members.

## File Layout

```
.weaver/
  issues/           # Issue markdown files
  hints/            # Hint markdown files
  workflows/        # Workflow YAML files
  launches/         # Agent launch logs
  index.yml         # Issue index (auto-generated, gitignored)
  hints_index.yml   # Hints index (auto-generated)
```

## CLI Reference

### Issues

| Command | Description |
|---------|-------------|
| `weaver create "Title" [options]` | Create a new issue |
| `weaver show <id>` | Show issue details |
| `weaver show <id> --fetch-deps` | Show issue with transitive dependencies |
| `weaver list [options]` | List all open issues |
| `weaver list -a` | List all issues including closed |
| `weaver list -s <status>` | Filter by status (open, in_progress, closed) |
| `weaver list -l <label>` | Filter by label |
| `weaver list -t <type>` | Filter by type |
| `weaver ready [options]` | List unblocked issues ready for work |
| `weaver ready -l <label>` | Filter ready queue by label |
| `weaver ready -t <type>` | Filter ready queue by type |
| `weaver ready -n <limit>` | Limit results |
| `weaver start <id>` | Mark issue as in_progress |
| `weaver close <id>` | Close issue |

**Create options:**
```
-t, --type TYPE          # task, bug, feature, epic, chore
-p, --priority 0-4       # 0=critical, 1=high, 2=medium (default), 3=low, 4=trivial
-l, --label LABEL        # Add label (repeatable)
-b, --blocked-by ID      # Add blocker (repeatable)
--parent ID              # Parent epic
-d, --description TEXT   # Short description
-f, --file PATH          # Read description from file ('-' for stdin)
```

### Dependencies

| Command | Description |
|---------|-------------|
| `weaver dep add <child> <parent>` | Add dependency (child blocked by parent) |
| `weaver dep rm <child> <parent>` | Remove dependency |

### Hints

| Command | Description |
|---------|-------------|
| `weaver hint add <title> -f <file>` | Add or update a hint from file |
| `weaver hint show <title_or_id>` | Show hint content |
| `weaver hint list` | List all hints |
| `weaver hint search <query>` | Search hints by content |

### Workflows

| Command | Description |
|---------|-------------|
| `weaver workflow create <name> -f <file>` | Create workflow template from YAML file |
| `weaver workflow execute <name>` | Create issues from workflow |
| `weaver workflow list` | List all workflows |
| `weaver workflow show <name>` | Show workflow details |

### Agent Launch

| Command | Description |
|---------|-------------|
| `weaver launch <id> [--model sonnet\|opus\|flash]` | Launch autonomous Claude agent on issue |

### Sync

| Command | Description |
|---------|-------------|
| `weaver sync` | Sync to `weaver-<username>` branch |
| `weaver sync -b <branch>` | Sync to specified branch |
| `weaver sync --push` | Push changes after sync |
| `weaver sync --pull` | Pull changes before sync |

### Other

| Command | Description |
|---------|-------------|
| `weaver init` | Initialize project |
| `weaver readme` | Display this guide |

## Example: Issue Storage Format

Issues are stored as markdown files in `.weaver/issues/`:

```markdown
---
id: wv-a3f8
title: Implement user authentication
type: feature
status: open
priority: 1
labels: [backend, security]
blocked_by: []
parent: null
created_at: 2025-01-06T10:00:00
updated_at: 2025-01-06T10:00:00
---

**Goal**: Add JWT-based authentication to API endpoints.

**Exit Conditions**:
- [ ] Login endpoint returns JWT
- [ ] Protected routes validate token
- [ ] Refresh token mechanism

**Related Code**:
- src/auth/token_manager.py
- src/api/endpoints.py

**Context**: Use pyjwt library. Token expiry: 24 hours.
```

## Best Practices & Workflows

### Core Principle: Always File Issues

**When you encounter new work, file a weaver issue immediately.** This includes:

- New features or tasks the user requests
- Bugs discovered during implementation
- Refactoring needed before proceeding
- Technical debt that blocks progress
- Research tasks to understand the codebase

**Don't just announce work - file it:**

```bash
# Bad: "I notice we should also update the tests"
# Good:
weaver create "Update tests for authentication changes" -t task -b wv-a3f8 -f- <<HERE
We're missing tests for the new authentication flow in src/auth/token_manager.py...
HERE
```

### Structuring Your Work

#### Break Down Large Tasks

When you receive a large task, break it into smaller issues with dependencies:

```bash
# Main feature
weaver create "Add user authentication" -t feature -p 1

# Subtasks (assuming the feature ID is wv-a3f8)
weaver create "Design auth token schema" -t task -b wv-a3f8 -p 1
weaver create "Implement login endpoint" -t task -b wv-b2c9 -p 1
weaver create "Add auth middleware" -t task -b wv-c1d4 -p 1
weaver create "Write auth tests" -t task -b wv-e5f6 -p 1
```

#### Use Dependencies to Show Order

Dependencies ensure you work on tasks in the right order:

```bash
# Task B depends on Task A completing first
weaver dep add wv-b2c9 wv-a3f8  # B is blocked by A
```

The `weaver ready` command automatically shows only unblocked tasks.

#### Write Clear Issue Descriptions

Follow this structure for every issue:

```markdown
**Goal**: [One sentence describing what to accomplish]

**Exit Conditions**:
- [ ] Specific, verifiable condition 1
- [ ] Specific, verifiable condition 2
- [ ] Tests pass

**Related Code**:
- path/to/file.py: `function_name()`, `ClassName`
- path/to/test.py

**Context**: [Optional background: why this is needed, constraints, design decisions]
```

Example:

```bash
cat <<'EOF' | weaver create "Add password hashing" -f -
**Goal**: Hash user passwords using bcrypt before storing in database.

**Exit Conditions**:
- [ ] Passwords are hashed with bcrypt in user creation flow
- [ ] Login verifies password against hash
- [ ] Migration script hashes existing plaintext passwords
- [ ] All auth tests pass

**Related Code**:
- src/auth/user_manager.py: `create_user()`, `verify_password()`
- src/auth/migrations/003_hash_passwords.py
- tests/test_auth.py

**Context**: Current implementation stores plaintext passwords. Need bcrypt
with work factor 12 for security compliance.
EOF
```

### Tips for Effective Use

#### File Issues During Discovery

As you explore code and discover work, file issues immediately:

```bash
# While implementing feature A, you notice B needs fixing
weaver create "Fix error handling in token refresh" -t bug -p 1 -l auth

# You realize C is needed
weaver create "Add rate limiting to auth endpoints" -t task -p 2 -l auth -l security
```

#### Use Labels for Context

Labels help filter and organize:

```bash
weaver create "Title" -l backend -l auth -l urgent
weaver ready -l backend  # Show only backend tasks
```

Common labels:
- Component: `backend`, `frontend`, `api`, `cli`
- Type: `bug`, `refactor`, `docs`, `tests`
- Priority: `urgent`, `blocked`, `tech-debt`

#### Track Blockers

If you're blocked on external input or another task:

```bash
# Create blocker task
weaver create "Research best practice for JWT expiry" -t task -p 1

# Link your task to the blocker (assuming blocker is wv-z9x8)
weaver dep add wv-a3f8 wv-z9x8  # wv-a3f8 is blocked by wv-z9x8
```

#### Use --fetch-deps for Context

When starting complex tasks, view all dependencies:

```bash
weaver show wv-xxxx --fetch-deps
```

This shows the full context: what this task depends on, what's already done, and the complete task graph.

### Priority Levels

- **P0**: Critical - System is broken, blocking all work
- **P1**: High - Important feature or serious bug
- **P2**: Medium - Standard tasks (default)
- **P3**: Low - Nice to have
- **P4**: Trivial - Cleanup, minor improvements

### Common Patterns

#### Bug Fix Workflow

```bash
# 1. File the bug
cat <<'EOF' | weaver create "Fix race condition in token refresh" -t bug -p 0 -f -
**Goal**: Prevent multiple simultaneous token refreshes when token expires.

**Exit Conditions**:
- [ ] Only one refresh occurs when multiple requests happen simultaneously
- [ ] Other requests wait for the single refresh to complete
- [ ] Tests cover the race condition

**Related Code**:
- src/auth/token_manager.py: `refresh_token()`, `get_valid_token()`
- tests/test_auth.py

**Context**: Multiple tabs trigger multiple refreshes, causing 401 errors.
EOF

# 2. Start it
weaver start wv-xxxx

# 3. Fix it
# ... make changes ...

# 4. Close it
weaver close wv-xxxx
```

#### Feature Implementation

```bash
# 1. Create epic
weaver create "User authentication system" -t epic -p 1

# 2. Break into tasks (assuming epic is wv-e1e1)
weaver create "Design auth schema" -t task --parent wv-e1e1 -p 1
weaver create "Implement JWT generation" -t task --parent wv-e1e1 -p 1
weaver create "Add login endpoint" -t task --parent wv-e1e1 -p 1
weaver create "Add auth middleware" -t task --parent wv-e1e1 -p 1
weaver create "Write tests" -t task --parent wv-e1e1 -p 1

# 3. Add dependencies
# (each task depends on the previous one)

# 4. Work through the ready queue
weaver ready --parent wv-e1e1
```

### Remember

1. **File issues, don't just announce them** - Create trackable work items
2. **Use dependencies** - Let weaver compute the ready queue
3. **Write clear exit conditions** - Make completion criteria explicit
4. **Label everything** - Enable filtering and organization
5. **Check `weaver ready` often** - Stay focused on unblocked work
6. **Close issues when done** - Keep the ready queue accurate

Weaver helps you maintain context, structure work, and avoid forgetting tasks. Use it liberally.
