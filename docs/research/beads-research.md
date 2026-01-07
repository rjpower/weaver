# Research Report: Beads LLM Issue Tracker and Alternatives

## 1. Problem Domain

**Goal**: Build a Python CLI issue tracker for AI coding agents using markdown files instead of JSONL, with modern UV-based tooling.

### Requirements Gathered

**Must-have:**
- Dependency-aware issue graph (blocking relationships)
- Git-friendly storage (human-readable, mergeable)
- CLI interface for agents and humans
- Issue statuses and priority levels
- Hierarchical issues (epics → tasks → subtasks)

**Nice-to-have:**
- Ready queue (auto-detect unblocked tasks)
- Audit trail / event history
- Content hashing for deduplication
- MCP server integration

**Constraints:**
- Python with UV tooling
- Markdown files (not JSONL)
- Directory-based storage (not single file)

---

## 2. Candidate Analysis

### 2.1 Beads (steveyegge/beads)

**Source**: [github.com/steveyegge/beads](https://github.com/steveyegge/beads)

**Overview**: The original distributed, git-backed graph issue tracker for AI agents. Created by Steve Yegge.

**Architecture:**
- **Dual storage**: SQLite (query cache) + JSONL (source of truth in `.beads/`)
- **Hash-based IDs**: Format `bd-a3f8` prevents merge collisions
- **Hierarchical**: `bd-a3f8` (epic) → `bd-a3f8.1` (task) → `bd-a3f8.1.1` (subtask)
- **Background daemon**: RPC over Unix socket, auto-sync with git

**Issue Schema (29 fields):**
```
ID, ContentHash, Title, Description, Design, Notes
Status: open | in_progress | blocked | closed | deferred | tombstone
Priority: 0-4 (P0 = critical)
IssueType: task | bug | feature | chore | epic | molecule | gate | convoy
CreatedAt, UpdatedAt, ClosedAt, DeletedAt
Dependencies, Labels, AcceptanceCriteria
```

**Dependency Types:**
| Type | Effect | Purpose |
|------|--------|---------|
| `blocks` | Affects ready | Hard blocker |
| `parent-child` | Affects ready | Epic/subtask hierarchy |
| `waits-for` | Affects ready | Dynamic fanout |
| `related` | Informational | Association |
| `duplicates` | Informational | Dedup tracking |

**Key Commands:**
```bash
bd ready              # List unblocked tasks
bd create "Title" -p 0  # Create priority-0 task
bd dep add <child> <parent>  # Create dependency
bd show <id>          # View details
bd list --status open  # Filter by status
```

**Strengths:**
- Mature, battle-tested with real agents
- Sophisticated dependency DAG with cycle prevention
- "Memory decay" summarizes old closed tasks
- Multiple UI projects: beads_viewer, beady, beads-tui

**Weaknesses:**
- Written in Go (not Python)
- JSONL format is less human-readable than markdown
- Complex SQLite cache adds conceptual overhead
- Daemon architecture may be overkill for simpler use cases

---

### 2.2 Backlog.md (MrLesk/Backlog.md)

**Source**: [github.com/MrLesk/Backlog.md](https://github.com/MrLesk/Backlog.md)

**Overview**: Turns any git repo into a project board with plain markdown files.

**Architecture:**
- **Directory storage**: `backlog/task-<id> - <title>.md`
- **YAML frontmatter**: Metadata in each markdown file
- **No database**: Pure flat files
- **MCP support**: Built-in server for AI agent integration

**File Format:**
```markdown
---
status: "In Progress"
priority: high
assignees: [alice]
labels: [backend, api]
dependencies: [task-1, task-5]
parent: task-10
---

# Task Title

Description here...

## Acceptance Criteria
- [ ] First criterion
- [ ] Second criterion
```

**Key Commands:**
```bash
backlog init          # Initialize project
backlog task create "Title" --priority high
backlog task list --status "In Progress"
backlog board         # Interactive Kanban TUI
backlog mcp start     # Start MCP server
```

**Strengths:**
- Pure markdown (extremely human-readable)
- Individual files per issue (clean git diffs)
- Built-in MCP server
- No database complexity

**Weaknesses:**
- Node.js/TypeScript (not Python)
- Less sophisticated dependency handling
- No ready queue computation
- No content hashing or dedup

---

### 2.3 TrackDown (mgoellnitz/trackdown)

**Source**: [github.com/mgoellnitz/trackdown](https://github.com/mgoellnitz/trackdown)

**Overview**: Lightweight issue tracking in a single markdown file.

**Architecture:**
- **Single file**: All issues in one markdown document
- **Git hooks**: Post-commit hook updates issues from commit messages
- **Markdown format**: Human-readable issue lists

**Strengths:**
- Extremely simple (single file)
- Git hook integration

**Weaknesses:**
- No dependency tracking
- Single file doesn't scale
- Limited metadata support

---

### 2.4 Tasks.md (BaldissaraMatheus/Tasks.md)

**Source**: [github.com/BaldissaraMatheus/Tasks.md](https://github.com/BaldissaraMatheus/Tasks.md)

**Overview**: Self-hosted Kanban board using markdown files.

**Strengths:**
- Web UI focused
- PWA support

**Weaknesses:**
- Web-first, not CLI-first
- No dependency graph
- Not designed for AI agents

---

### 2.5 Sciit (Source Control Integrated Issue Tracker)

**Source**: [sciit.gitlab.io/sciit](https://sciit.gitlab.io/sciit/)

**Overview**: Issues as comments embedded in source code.

**Strengths:**
- Issues live next to code

**Weaknesses:**
- Invasive (modifies source files)
- Not suitable for standalone tracking

---

## 3. Comparison Matrix

| Feature | Beads | Backlog.md | TrackDown | Tasks.md |
|---------|-------|------------|-----------|----------|
| **Language** | Go | TypeScript | Java | TypeScript |
| **Storage** | JSONL+SQLite | Markdown files | Single MD | Markdown |
| **Dependency Graph** | ★★★★★ | ★★☆☆☆ | ☆☆☆☆☆ | ☆☆☆☆☆ |
| **Ready Queue** | ★★★★★ | ☆☆☆☆☆ | ☆☆☆☆☆ | ☆☆☆☆☆ |
| **Human Readable** | ★★☆☆☆ | ★★★★★ | ★★★★☆ | ★★★★☆ |
| **Git Merge** | ★★★★☆ | ★★★★★ | ★★★☆☆ | ★★★★☆ |
| **AI Agent Focus** | ★★★★★ | ★★★★☆ | ★☆☆☆☆ | ★☆☆☆☆ |
| **MCP Support** | ★★★★☆ | ★★★★★ | ☆☆☆☆☆ | ☆☆☆☆☆ |
| **Maintenance** | Active | Active | Low | Moderate |

---

## 4. Design Insights for Python Implementation

### Storage Format: Directory of Markdown Files

**Proposed structure:**
```
.weaver/
├── config.yml           # Project configuration
├── issues/
│   ├── bd-a3f8.md       # Individual issue files
│   ├── bd-b2c9.md
│   └── bd-c4d1.md
└── index.yml            # Lightweight index for fast queries
```

**Issue File Format:**
```markdown
---
id: bd-a3f8
title: Implement user authentication
type: feature
status: open
priority: 1
created: 2025-01-06T10:00:00Z
updated: 2025-01-06T10:00:00Z
labels: [backend, security]
blocks: []
blocked_by: [bd-b2c9]
parent: null
---

# Implement user authentication

## Description
Add JWT-based authentication to the API endpoints.

## Design Notes
Use pyjwt library. Token expiry: 24 hours.

## Acceptance Criteria
- [ ] Login endpoint returns JWT
- [ ] Protected routes validate token
- [ ] Refresh token mechanism

## History
- 2025-01-06: Created (agent)
```

### Key Architectural Decisions

1. **No SQLite**: Use YAML index file for fast lookups, rebuild on startup
2. **Content Hash**: SHA256 of title + description + design for dedup
3. **Lazy DAG**: Build dependency graph in-memory when needed
4. **Atomic writes**: Write to temp file, then rename

### Core Data Model (Python)

```python
from enum import Enum
from dataclasses import dataclass
from datetime import datetime

class Status(Enum):
    OPEN = "open"
    IN_PROGRESS = "in_progress"
    BLOCKED = "blocked"
    CLOSED = "closed"
    DEFERRED = "deferred"

class IssueType(Enum):
    TASK = "task"
    BUG = "bug"
    FEATURE = "feature"
    EPIC = "epic"
    CHORE = "chore"

@dataclass
class Issue:
    id: str
    title: str
    type: IssueType
    status: Status
    priority: int  # 0-4
    description: str
    design_notes: str | None
    acceptance_criteria: list[str]
    labels: list[str]
    blocks: list[str]       # IDs this issue blocks
    blocked_by: list[str]   # IDs blocking this issue
    parent: str | None      # Epic ID
    created_at: datetime
    updated_at: datetime
    closed_at: datetime | None
    content_hash: str
```

### CLI Design (Click-based)

```python
@click.group()
def cli():
    """Weaver - Issue tracking for AI agents"""

@cli.command()
def ready():
    """List unblocked tasks ready for work"""

@cli.command()
@click.argument("title")
@click.option("-p", "--priority", type=int, default=2)
@click.option("-t", "--type", type=click.Choice(["task", "bug", "feature"]))
def create(title: str, priority: int, type: str):
    """Create a new issue"""

@cli.command()
@click.argument("child_id")
@click.argument("parent_id")
def block(child_id: str, parent_id: str):
    """Mark child as blocked by parent"""

@cli.command()
@click.argument("issue_id")
def show(issue_id: str):
    """Show issue details"""

@cli.command()
@click.option("--status", type=click.Choice(["open", "closed", "all"]))
def list(status: str):
    """List issues"""
```

---

## 5. Recommendation

### Approach: Beads-inspired design with Backlog.md-style storage

**Rationale:**
1. **Beads' dependency model is the gold standard** for AI agent task management
2. **Markdown files are more human-friendly** than JSONL
3. **Directory structure enables clean git diffs** (one file per issue)
4. **Python + UV** is the target stack (neither beads nor Backlog.md satisfy this)

### Key Features to Implement

| Priority | Feature | Source |
|----------|---------|--------|
| P0 | Dependency graph with `blocks`/`blocked_by` | Beads |
| P0 | `ready` command (unblocked tasks) | Beads |
| P0 | Markdown file storage | Backlog.md |
| P1 | Hash-based IDs | Beads |
| P1 | Hierarchical issues (parent/child) | Beads |
| P1 | YAML frontmatter | Backlog.md |
| P2 | Content hashing for dedup | Beads |
| P2 | MCP server | Backlog.md |
| P3 | Event history / audit trail | Beads |

### Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Index file staleness | Rebuild index on startup, validate on read |
| Markdown parsing edge cases | Use python-frontmatter library |
| Dependency cycle creation | Validate DAG before write |
| Performance at scale | Add optional SQLite cache later if needed |

---

## 6. Sources

- [Beads - steveyegge/beads](https://github.com/steveyegge/beads)
- [Beads Viewer - Dicklesworthstone/beads_viewer](https://github.com/Dicklesworthstone/beads_viewer)
- [Beady - maphew/beady](https://github.com/maphew/beady)
- [DeepWiki: Beads Architecture](https://deepwiki.com/steveyegge/beads)
- [Backlog.md - MrLesk/Backlog.md](https://github.com/MrLesk/Backlog.md)
- [TrackDown - mgoellnitz/trackdown](https://github.com/mgoellnitz/trackdown)
- [Sciit Documentation](https://sciit.gitlab.io/sciit/)
- [Tasks.md - BaldissaraMatheus/Tasks.md](https://github.com/BaldissaraMatheus/Tasks.md)
- [Better Stack: Beads Guide](https://betterstack.com/community/guides/ai/beads-issue-tracker-ai-agents/)
