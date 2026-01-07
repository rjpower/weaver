# Weaver Issue Tracker - Implementation Spec

## 1. Context Summary

**Problem**: AI coding agents need persistent, structured memory for long-horizon tasks. Current solutions (beads) use JSONL which is less human-readable, or lack sophisticated dependency tracking (Backlog.md).

**Selected Approach**: Python CLI using directory of markdown files with YAML frontmatter, inspired by beads' dependency model and Backlog.md's storage approach.

**Research Reference**: `docs/research/beads-research.md`

**Key Decisions**:
- Flat IDs only (no hierarchical numbering)
- CLI-first (no MCP server)
- No event history/audit trail for v1
- `ready` command supports filtering by labels

---

## 2. System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      CLI (Click)                        │
│  init | create | show | list | ready | close | dep      │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│                    Issue Service                         │
│  create_issue | update_issue | get_issue | list_issues  │
│  add_dependency | remove_dependency | get_ready_issues  │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│                   Dependency Graph                       │
│  build_graph | topological_sort | detect_cycles         │
│  get_blockers | get_blocked_by | is_blocked             │
└──────────────────────────┬──────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────┐
│                 Markdown Storage                         │
│  read_issue | write_issue | delete_issue | list_files   │
└──────────────────────────┬──────────────────────────────┘
                           │
                    .weaver/issues/*.md
```

**External Dependencies**:
- `click` - CLI framework
- `pyyaml` - YAML parsing
- `python-frontmatter` - Markdown frontmatter parsing

---

## 3. Data Models

### 3.1 Issue Model

```python
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Self
import hashlib


class Status(Enum):
    OPEN = "open"
    IN_PROGRESS = "in_progress"
    BLOCKED = "blocked"
    CLOSED = "closed"


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
    status: Status = Status.OPEN
    type: IssueType = IssueType.TASK
    priority: int = 2  # 0-4, lower = higher priority
    description: str = ""
    design_notes: str = ""
    acceptance_criteria: list[str] = field(default_factory=list)
    labels: list[str] = field(default_factory=list)
    blocked_by: list[str] = field(default_factory=list)  # IDs blocking this
    parent: str | None = None  # Epic ID
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    closed_at: datetime | None = None

    @property
    def content_hash(self) -> str:
        """SHA256 of title + description + design for dedup detection."""
        content = f"{self.title}|{self.description}|{self.design_notes}"
        return hashlib.sha256(content.encode()).hexdigest()[:12]

    def is_open(self) -> bool:
        return self.status in (Status.OPEN, Status.IN_PROGRESS, Status.BLOCKED)
```

### 3.2 ID Generation

```python
import secrets

def generate_id(prefix: str = "wv") -> str:
    """Generate a short hash-based ID like 'wv-a3f8'."""
    return f"{prefix}-{secrets.token_hex(2)}"
```

### 3.3 Markdown File Format

```markdown
---
id: wv-a3f8
title: Implement user authentication
type: feature
status: open
priority: 1
labels: [backend, security]
blocked_by: [wv-b2c9]
parent: wv-epic1
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

---

## 4. Component Design

### 4.1 Storage Layer (`weaver/storage.py`)

```python
from pathlib import Path
import frontmatter


class MarkdownStorage:
    def __init__(self, root: Path):
        self.root = root
        self.issues_dir = root / "issues"

    def ensure_initialized(self) -> None:
        """Create .weaver/issues if not exists."""
        self.issues_dir.mkdir(parents=True, exist_ok=True)

    def issue_path(self, issue_id: str) -> Path:
        return self.issues_dir / f"{issue_id}.md"

    def read_issue(self, issue_id: str) -> Issue | None:
        """Read and parse a single issue file."""
        path = self.issue_path(issue_id)
        if not path.exists():
            return None
        post = frontmatter.load(path)
        return self._parse_issue(post)

    def write_issue(self, issue: Issue) -> None:
        """Write issue to markdown file."""
        path = self.issue_path(issue.id)
        content = self._serialize_issue(issue)
        path.write_text(content)

    def delete_issue(self, issue_id: str) -> bool:
        """Delete issue file. Returns True if deleted."""
        path = self.issue_path(issue_id)
        if path.exists():
            path.unlink()
            return True
        return False

    def list_issue_ids(self) -> list[str]:
        """List all issue IDs from filenames."""
        return [p.stem for p in self.issues_dir.glob("*.md")]

    def read_all_issues(self) -> list[Issue]:
        """Read all issues from storage."""
        return [
            issue for issue_id in self.list_issue_ids()
            if (issue := self.read_issue(issue_id)) is not None
        ]
```

### 4.2 Dependency Graph (`weaver/graph.py`)

```python
from collections import defaultdict
from dataclasses import dataclass


@dataclass
class DependencyGraph:
    """DAG of issue dependencies for computing ready queue."""

    # issue_id -> list of IDs it's blocked by
    blocked_by: dict[str, set[str]]
    # issue_id -> list of IDs it blocks
    blocks: dict[str, set[str]]
    # All known issue IDs
    all_ids: set[str]

    @classmethod
    def build(cls, issues: list[Issue]) -> "DependencyGraph":
        blocked_by = defaultdict(set)
        blocks = defaultdict(set)
        all_ids = set()

        for issue in issues:
            all_ids.add(issue.id)
            for blocker_id in issue.blocked_by:
                blocked_by[issue.id].add(blocker_id)
                blocks[blocker_id].add(issue.id)

        return cls(
            blocked_by=dict(blocked_by),
            blocks=dict(blocks),
            all_ids=all_ids,
        )

    def is_blocked(self, issue_id: str, open_ids: set[str]) -> bool:
        """Check if issue is blocked by any open issues."""
        blockers = self.blocked_by.get(issue_id, set())
        return bool(blockers & open_ids)

    def get_unblocked(self, open_issues: list[Issue]) -> list[Issue]:
        """Return open issues not blocked by other open issues."""
        open_ids = {i.id for i in open_issues if i.is_open()}
        return [
            issue for issue in open_issues
            if issue.is_open()
            and issue.status != Status.BLOCKED
            and not self.is_blocked(issue.id, open_ids)
        ]

    def detect_cycle(self, from_id: str, to_id: str) -> bool:
        """Check if adding from_id -> to_id dependency creates a cycle."""
        # DFS from to_id to see if we can reach from_id
        visited = set()
        stack = [to_id]
        while stack:
            current = stack.pop()
            if current == from_id:
                return True
            if current in visited:
                continue
            visited.add(current)
            stack.extend(self.blocked_by.get(current, []))
        return False
```

### 4.3 Issue Service (`weaver/service.py`)

```python
class IssueService:
    def __init__(self, storage: MarkdownStorage):
        self.storage = storage
        self._graph: DependencyGraph | None = None

    def _invalidate_graph(self) -> None:
        self._graph = None

    def _get_graph(self) -> DependencyGraph:
        if self._graph is None:
            issues = self.storage.read_all_issues()
            self._graph = DependencyGraph.build(issues)
        return self._graph

    def create_issue(
        self,
        title: str,
        type: IssueType = IssueType.TASK,
        priority: int = 2,
        description: str = "",
        labels: list[str] | None = None,
        blocked_by: list[str] | None = None,
        parent: str | None = None,
    ) -> Issue:
        issue = Issue(
            id=generate_id(),
            title=title,
            type=type,
            priority=priority,
            description=description,
            labels=labels or [],
            blocked_by=blocked_by or [],
            parent=parent,
        )
        self.storage.write_issue(issue)
        self._invalidate_graph()
        return issue

    def get_issue(self, issue_id: str) -> Issue | None:
        return self.storage.read_issue(issue_id)

    def update_issue(self, issue: Issue) -> None:
        issue.updated_at = datetime.now()
        self.storage.write_issue(issue)
        self._invalidate_graph()

    def close_issue(self, issue_id: str) -> Issue | None:
        issue = self.get_issue(issue_id)
        if issue is None:
            return None
        issue.status = Status.CLOSED
        issue.closed_at = datetime.now()
        self.update_issue(issue)
        return issue

    def add_dependency(self, issue_id: str, blocked_by_id: str) -> bool:
        """Add dependency. Returns False if would create cycle."""
        graph = self._get_graph()
        if graph.detect_cycle(issue_id, blocked_by_id):
            return False

        issue = self.get_issue(issue_id)
        if issue is None:
            return False
        if blocked_by_id not in issue.blocked_by:
            issue.blocked_by.append(blocked_by_id)
            self.update_issue(issue)
        return True

    def list_issues(
        self,
        status: Status | None = None,
        labels: list[str] | None = None,
        type: IssueType | None = None,
    ) -> list[Issue]:
        """List issues with optional filters."""
        issues = self.storage.read_all_issues()

        if status is not None:
            issues = [i for i in issues if i.status == status]
        if labels:
            label_set = set(labels)
            issues = [i for i in issues if label_set & set(i.labels)]
        if type is not None:
            issues = [i for i in issues if i.type == type]

        return sorted(issues, key=lambda i: (i.priority, i.created_at))

    def get_ready_issues(
        self,
        labels: list[str] | None = None,
        type: IssueType | None = None,
        limit: int | None = None,
    ) -> list[Issue]:
        """Get unblocked issues ready for work, with filters."""
        issues = self.storage.read_all_issues()
        open_issues = [i for i in issues if i.is_open()]

        graph = self._get_graph()
        ready = graph.get_unblocked(open_issues)

        # Apply filters
        if labels:
            label_set = set(labels)
            ready = [i for i in ready if label_set & set(i.labels)]
        if type is not None:
            ready = [i for i in ready if i.type == type]

        # Sort by priority, then creation date
        ready = sorted(ready, key=lambda i: (i.priority, i.created_at))

        if limit:
            ready = ready[:limit]

        return ready
```

### 4.4 CLI (`weaver/cli.py`)

```python
import click
from rich.console import Console
from rich.table import Table

console = Console()


@click.group()
@click.pass_context
def cli(ctx: click.Context) -> None:
    """Weaver - Issue tracking for AI coding agents."""
    ctx.ensure_object(dict)
    root = find_weaver_root()
    if root is None and ctx.invoked_subcommand != "init":
        raise click.ClickException("Not in a weaver project. Run 'weaver init' first.")
    ctx.obj["service"] = IssueService(MarkdownStorage(root)) if root else None


@cli.command()
def init() -> None:
    """Initialize a new weaver project."""
    root = Path.cwd() / ".weaver"
    storage = MarkdownStorage(root)
    storage.ensure_initialized()
    console.print(f"Initialized weaver in {root}")


@cli.command()
@click.argument("title")
@click.option("-t", "--type", "issue_type",
              type=click.Choice(["task", "bug", "feature", "epic", "chore"]),
              default="task")
@click.option("-p", "--priority", type=click.IntRange(0, 4), default=2)
@click.option("-l", "--label", "labels", multiple=True)
@click.option("-b", "--blocked-by", "blocked_by", multiple=True)
@click.option("--parent", help="Parent epic ID")
@click.pass_context
def create(ctx, title, issue_type, priority, labels, blocked_by, parent) -> None:
    """Create a new issue."""
    service: IssueService = ctx.obj["service"]
    issue = service.create_issue(
        title=title,
        type=IssueType(issue_type),
        priority=priority,
        labels=list(labels),
        blocked_by=list(blocked_by),
        parent=parent,
    )
    console.print(f"Created {issue.id}: {issue.title}")


@cli.command()
@click.argument("issue_id")
@click.pass_context
def show(ctx, issue_id: str) -> None:
    """Show issue details."""
    service: IssueService = ctx.obj["service"]
    issue = service.get_issue(issue_id)
    if issue is None:
        raise click.ClickException(f"Issue {issue_id} not found")

    console.print(f"[bold]{issue.id}[/bold]: {issue.title}")
    console.print(f"Status: {issue.status.value}  Priority: P{issue.priority}  Type: {issue.type.value}")
    if issue.labels:
        console.print(f"Labels: {', '.join(issue.labels)}")
    if issue.blocked_by:
        console.print(f"Blocked by: {', '.join(issue.blocked_by)}")
    if issue.description:
        console.print(f"\n{issue.description}")


@cli.command("list")
@click.option("-s", "--status",
              type=click.Choice(["open", "in_progress", "blocked", "closed"]))
@click.option("-l", "--label", "labels", multiple=True)
@click.option("-t", "--type", "issue_type",
              type=click.Choice(["task", "bug", "feature", "epic", "chore"]))
@click.pass_context
def list_issues(ctx, status, labels, issue_type) -> None:
    """List issues with optional filters."""
    service: IssueService = ctx.obj["service"]
    issues = service.list_issues(
        status=Status(status) if status else None,
        labels=list(labels) if labels else None,
        type=IssueType(issue_type) if issue_type else None,
    )
    _print_issue_table(issues)


@cli.command()
@click.option("-l", "--label", "labels", multiple=True,
              help="Filter by label (can specify multiple)")
@click.option("-t", "--type", "issue_type",
              type=click.Choice(["task", "bug", "feature", "epic", "chore"]),
              help="Filter by issue type")
@click.option("-n", "--limit", type=int, help="Max number of issues to show")
@click.pass_context
def ready(ctx, labels, issue_type, limit) -> None:
    """List unblocked issues ready for work."""
    service: IssueService = ctx.obj["service"]
    issues = service.get_ready_issues(
        labels=list(labels) if labels else None,
        type=IssueType(issue_type) if issue_type else None,
        limit=limit,
    )
    if not issues:
        console.print("No ready issues found.")
        return
    _print_issue_table(issues)


@cli.command()
@click.argument("issue_id")
@click.pass_context
def close(ctx, issue_id: str) -> None:
    """Close an issue."""
    service: IssueService = ctx.obj["service"]
    issue = service.close_issue(issue_id)
    if issue is None:
        raise click.ClickException(f"Issue {issue_id} not found")
    console.print(f"Closed {issue.id}: {issue.title}")


@cli.group()
def dep() -> None:
    """Manage issue dependencies."""


@dep.command("add")
@click.argument("issue_id")
@click.argument("blocked_by_id")
@click.pass_context
def dep_add(ctx, issue_id: str, blocked_by_id: str) -> None:
    """Mark ISSUE_ID as blocked by BLOCKED_BY_ID."""
    service: IssueService = ctx.obj["service"]
    if not service.add_dependency(issue_id, blocked_by_id):
        raise click.ClickException("Cannot add dependency: would create cycle or issue not found")
    console.print(f"{issue_id} is now blocked by {blocked_by_id}")


def _print_issue_table(issues: list[Issue]) -> None:
    table = Table()
    table.add_column("ID", style="cyan")
    table.add_column("P", justify="center")
    table.add_column("Status")
    table.add_column("Type")
    table.add_column("Title")
    table.add_column("Labels")

    for issue in issues:
        table.add_row(
            issue.id,
            str(issue.priority),
            issue.status.value,
            issue.type.value,
            issue.title[:50],
            ", ".join(issue.labels),
        )
    console.print(table)
```

---

## 5. Project Structure

```
weaver/
├── pyproject.toml
├── src/
│   └── weaver/
│       ├── __init__.py
│       ├── cli.py          # Click commands
│       ├── service.py      # Business logic
│       ├── storage.py      # Markdown file I/O
│       ├── graph.py        # Dependency DAG
│       └── models.py       # Issue, Status, IssueType
└── tests/
    ├── conftest.py         # Fixtures (tmp_path weaver root)
    ├── test_storage.py
    ├── test_graph.py
    ├── test_service.py
    └── test_cli.py
```

---

## 6. Implementation Stages

### Stage 1: Data Models & Storage
**Goal**: Read/write issues as markdown files

**Components**:
- `models.py`: Issue, Status, IssueType dataclasses
- `storage.py`: MarkdownStorage class

**Tests**:
- Round-trip: create Issue → write → read → compare
- List issue IDs from directory
- Handle missing files gracefully
- Parse YAML frontmatter correctly

### Stage 2: Dependency Graph
**Goal**: Build DAG and compute blocked/ready status

**Components**:
- `graph.py`: DependencyGraph class

**Tests**:
- Build graph from issues with dependencies
- `is_blocked` returns True when blocker is open
- `is_blocked` returns False when blocker is closed
- `get_unblocked` filters correctly
- `detect_cycle` catches cycles
- `detect_cycle` allows valid deps

### Stage 3: Issue Service
**Goal**: Business logic layer with filtering

**Components**:
- `service.py`: IssueService class

**Tests**:
- `create_issue` generates ID and persists
- `close_issue` updates status and timestamp
- `add_dependency` rejects cycles
- `list_issues` filters by status, labels, type
- `get_ready_issues` combines graph + filters

### Stage 4: CLI Commands
**Goal**: Complete CLI with all commands

**Components**:
- `cli.py`: Click command group

**Tests**:
- `weaver init` creates `.weaver/issues/`
- `weaver create "title"` creates issue file
- `weaver show <id>` displays issue
- `weaver list` shows table
- `weaver ready -l backend` filters by label
- `weaver close <id>` closes issue
- `weaver dep add <child> <parent>` adds dependency

---

## 7. CLI Reference

```
weaver init                           # Initialize project
weaver create "Title" [options]       # Create issue
  -t, --type TYPE                     # task|bug|feature|epic|chore
  -p, --priority 0-4                  # Priority (0=critical)
  -l, --label LABEL                   # Add label (repeatable)
  -b, --blocked-by ID                 # Add blocker (repeatable)
  --parent ID                         # Parent epic

weaver show ID                        # Show issue details
weaver list [options]                 # List issues
  -s, --status STATUS                 # Filter by status
  -l, --label LABEL                   # Filter by label
  -t, --type TYPE                     # Filter by type

weaver ready [options]                # List ready issues
  -l, --label LABEL                   # Filter by label
  -t, --type TYPE                     # Filter by type
  -n, --limit N                       # Max results

weaver close ID                       # Close issue
weaver dep add CHILD PARENT           # Add dependency
weaver dep rm CHILD PARENT            # Remove dependency
```

---

## 8. Dependencies (pyproject.toml)

```toml
[project]
name = "weaver"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "click>=8.1",
    "python-frontmatter>=1.1",
    "pyyaml>=6.0",
    "rich>=13.0",
]

[project.scripts]
weaver = "weaver.cli:cli"

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[tool.uv]
dev-dependencies = [
    "pytest>=8.0",
    "pytest-cov>=4.0",
]
```

---

## 9. Open Questions

None - all decisions made:
- Flat IDs only ✓
- CLI-first, no MCP ✓
- No event history v1 ✓
- Ready supports label/type filters ✓
