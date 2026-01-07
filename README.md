# Weaver

Issue tracking for AI coding agents.

Weaver is a CLI tool that stores issues as markdown files with YAML frontmatter, making it easy for both humans and AI agents to read and modify issues. It features dependency tracking with cycle detection, automatic "ready queue" computation, and fast queries via a lightweight index.

## Installation

```bash
uv pip install -e .
```

## Quick Start

```bash
# Initialize a new weaver project
weaver init

# Create issues
weaver create "Implement user authentication" -t feature -p 1 -l backend -l security
weaver create "Add login endpoint" -t task -b wv-a3f8  # blocked by the feature

# Create issue with description from file or stdin
weaver create "Complex feature" -f description.md
cat spec.md | weaver create "Feature from spec" -f -

# View issues
weaver list                    # List all issues
weaver list -s open            # Filter by status
weaver list -l backend         # Filter by label
weaver ready                   # Show unblocked issues ready for work
weaver show wv-a3f8           # Show issue details

# Update status
weaver start wv-a3f8          # Mark as in_progress
weaver close wv-a3f8          # Close the issue

# Manage dependencies
weaver dep add wv-b2c9 wv-a3f8   # wv-b2c9 is blocked by wv-a3f8
weaver dep rm wv-b2c9 wv-a3f8    # Remove the dependency
```

## Storage Format

Issues are stored as markdown files in `.weaver/issues/`:

```markdown
---
id: wv-a3f8
title: Implement user authentication
type: feature
status: open
priority: 1
labels: [backend, security]
blocked_by: [wv-b2c9]
parent: null
created_at: 2025-01-06T10:00:00
updated_at: 2025-01-06T10:00:00
---

Add JWT-based authentication to the API endpoints.

## Design Notes

Use pyjwt library. Token expiry: 24 hours.

## Acceptance Criteria

- [ ] Login endpoint returns JWT
- [ ] Protected routes validate token
- [ ] Refresh token mechanism
```

## CLI Reference

```
weaver init                           # Initialize project
weaver create "Title" [options]       # Create issue
  -t, --type TYPE                     # task|bug|feature|epic|chore
  -p, --priority 0-4                  # Priority (0=critical)
  -l, --label LABEL                   # Add label (repeatable)
  -b, --blocked-by ID                 # Add blocker (repeatable)
  --parent ID                         # Parent epic
  -d, --description TEXT              # Issue description (short)
  -f, --file PATH                     # Read description from file ('-' for stdin)

weaver show ID                        # Show issue details
weaver list [options]                 # List issues
  -s, --status STATUS                 # Filter by status
  -l, --label LABEL                   # Filter by label
  -t, --type TYPE                     # Filter by type

weaver ready [options]                # List ready (unblocked) issues
  -l, --label LABEL                   # Filter by label
  -t, --type TYPE                     # Filter by type
  -n, --limit N                       # Max results

weaver start ID                       # Mark issue as in_progress
weaver close ID                       # Close issue
weaver dep add CHILD PARENT           # Add dependency
weaver dep rm CHILD PARENT            # Remove dependency
```

## Features

- **Markdown storage** - Human-readable, git-friendly issue files
- **Dependency tracking** - Block issues on other issues with cycle detection
- **Ready queue** - Automatically find unblocked issues ready for work
- **Fast queries** - Lightweight index.yml for quick filtering
- **Priority levels** - P0 (critical) through P4 (low)
- **Issue types** - task, bug, feature, epic, chore
- **Labels** - Tag issues for filtering

## Writing Good Issues

A well-written issue helps AI agents (and humans) understand what needs to be done. Structure issues like bug reports:

**Goal**: A concise 1-2 sentence description of what should be accomplished.

**Exit Conditions**: Concrete, verifiable criteria that indicate the issue is complete.

**Related Code**: File paths, function names, or modules relevant to the issue.

**Context** (optional): Background information, constraints, or design decisions.

Example:

```markdown
---
id: wv-a1b2
title: Fix token refresh race condition
type: bug
status: open
priority: 1
labels: [auth, backend]
---

**Goal**: Prevent concurrent API requests from triggering multiple token refreshes.

**Exit Conditions**:
- [ ] Only one refresh request occurs when token expires
- [ ] Concurrent requests wait for the single refresh to complete
- [ ] Tests cover the race condition scenario

**Related Code**:
- src/auth/token_manager.py: `refresh_token()`, `get_valid_token()`
- tests/test_auth.py

**Context**: Users report 401 errors when multiple tabs are open. The refresh
endpoint is being called multiple times simultaneously.
```

## Development

```bash
# Install dev dependencies
uv sync

# Run tests
uv run pytest

# Run tests with coverage
uv run pytest --cov=weaver
```
