# Weaver

Issue tracking designed for AI coding agents.

Weaver stores issues as markdown files with YAML frontmatter, making it easy for both humans and AI agents to read and modify issues. It features dependency tracking with cycle detection, automatic "ready queue" computation, workflows, hints, and autonomous agent launch.

> **For AI Agents**: See [AGENTS.md](./AGENTS.md) for essential guidance on structuring work, writing good issues, and using the ready queue effectively.

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
uv pip install -e .
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

## Additional Resources

- [AGENTS.md](./AGENTS.md) - Comprehensive guide for AI agents
- `.weaver/hints/` - Project-specific knowledge hints
- `weaver readme` - Display this guide in terminal

## Development

```bash
# Install dev dependencies
uv sync

# Run tests
uv run pytest

# Run tests with coverage
uv run pytest --cov=weaver
```
