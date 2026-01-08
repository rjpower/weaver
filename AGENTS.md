# Weaver for AI Agents

This guide helps AI coding agents use Weaver effectively to structure and track their work.

## Quick Start

Weaver is an issue tracker designed for AI agents. Use it to break down work, track dependencies, and maintain context across sessions.

```bash
# Check what's ready to work on
weaver ready

# Start working on an issue
weaver start wv-xxxx

# View issue with all dependencies
weaver show wv-xxxx --fetch-deps

# Complete the issue
weaver close wv-xxxx
```

## Core Principle: Always File Issues

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
weaver create "Update tests for authentication changes" -t task -b wv-a3f8
```

## Structuring Your Work

### Break Down Large Tasks

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

### Use Dependencies to Show Order

Dependencies ensure you work on tasks in the right order:

```bash
# Task B depends on Task A completing first
weaver dep add wv-b2c9 wv-a3f8  # B is blocked by A
```

The `weaver ready` command automatically shows only unblocked tasks.

### Write Clear Issue Descriptions

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

## Working with the Ready Queue

Use `weaver ready` to find your next task:

```bash
# Show all unblocked tasks
weaver ready

# Filter by label
weaver ready -l backend

# Limit results
weaver ready -n 3
```

**Workflow:**
1. `weaver ready` → Find unblocked task
2. `weaver show wv-xxxx --fetch-deps` → Review task + dependencies
3. `weaver start wv-xxxx` → Mark as in progress
4. Do the work
5. `weaver close wv-xxxx` → Mark complete
6. Repeat

## Tips for Effective Use

### File Issues During Discovery

As you explore code and discover work, file issues immediately:

```bash
# While implementing feature A, you notice B needs fixing
weaver create "Fix error handling in token refresh" -t bug -p 1 -l auth

# You realize C is needed
weaver create "Add rate limiting to auth endpoints" -t task -p 2 -l auth -l security
```

### Use Labels for Context

Labels help filter and organize:

```bash
weaver create "Title" -l backend -l auth -l urgent
weaver ready -l backend  # Show only backend tasks
```

Common labels:
- Component: `backend`, `frontend`, `api`, `cli`
- Type: `bug`, `refactor`, `docs`, `tests`
- Priority: `urgent`, `blocked`, `tech-debt`

### Track Blockers

If you're blocked on external input or another task:

```bash
# Create blocker task
weaver create "Research best practice for JWT expiry" -t task -p 1

# Link your task to the blocker (assuming blocker is wv-z9x8)
weaver dep add wv-a3f8 wv-z9x8  # wv-a3f8 is blocked by wv-z9x8
```

### Use --fetch-deps for Context

When starting complex tasks, view all dependencies:

```bash
weaver show wv-xxxx --fetch-deps
```

This shows the full context: what this task depends on, what's already done, and the complete task graph.

## Priority Levels

- **P0**: Critical - System is broken, blocking all work
- **P1**: High - Important feature or serious bug
- **P2**: Medium - Standard tasks (default)
- **P3**: Low - Nice to have
- **P4**: Trivial - Cleanup, minor improvements

## Common Patterns

### Bug Fix Workflow

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

### Feature Implementation

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

## Remember

1. **File issues, don't just announce them** - Create trackable work items
2. **Use dependencies** - Let weaver compute the ready queue
3. **Write clear exit conditions** - Make completion criteria explicit
4. **Label everything** - Enable filtering and organization
5. **Check `weaver ready` often** - Stay focused on unblocked work
6. **Close issues when done** - Keep the ready queue accurate

Weaver helps you maintain context, structure work, and avoid forgetting tasks. Use it liberally.
