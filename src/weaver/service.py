"""Business logic service for Weaver issue tracker."""

from datetime import datetime

from weaver.graph import DependencyGraph
from weaver.models import Issue, IssueType, Status, generate_id
from weaver.storage import MarkdownStorage


class IssueNotFoundError(Exception):
    """Raised when an issue is not found."""

    def __init__(self, issue_id: str):
        self.issue_id = issue_id
        super().__init__(f"Issue not found: {issue_id}")


class DependencyError(Exception):
    """Raised when a dependency operation fails."""

    pass


class IssueService:
    """Business logic layer for issue operations."""

    def __init__(self, storage: MarkdownStorage):
        self.storage = storage
        self._graph: DependencyGraph | None = None

    def _invalidate_graph(self) -> None:
        """Invalidate cached graph when issues change."""
        self._graph = None

    def _get_graph(self) -> DependencyGraph:
        """Get or build dependency graph."""
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
        """Create a new issue."""
        # Validate blocked_by references
        if blocked_by:
            for dep_id in blocked_by:
                if not self.validate_issue_exists(dep_id):
                    raise DependencyError(f"Cannot block by non-existent issue: {dep_id}")

        # Validate parent reference
        if parent and not self.validate_issue_exists(parent):
            raise DependencyError(f"Parent issue not found: {parent}")

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
        """Get an issue by ID."""
        return self.storage.read_issue(issue_id)

    def update_issue(self, issue: Issue) -> None:
        """Update an existing issue."""
        issue.updated_at = datetime.now()
        self.storage.write_issue(issue)
        self._invalidate_graph()

    def close_issue(self, issue_id: str) -> Issue:
        """Close an issue."""
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        issue.status = Status.CLOSED
        issue.closed_at = datetime.now()
        self.update_issue(issue)
        return issue

    def start_issue(self, issue_id: str) -> Issue:
        """Mark an issue as in progress."""
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        issue.status = Status.IN_PROGRESS
        self.update_issue(issue)
        return issue

    def validate_issue_exists(self, issue_id: str) -> bool:
        """Check if an issue exists."""
        return self.storage.read_issue(issue_id) is not None

    def add_dependency(self, issue_id: str, blocked_by_id: str) -> None:
        """
        Add dependency: issue_id becomes blocked by blocked_by_id.

        Raises:
            IssueNotFoundError: If either issue doesn't exist.
            DependencyError: If adding the dependency would create a cycle.
        """
        # Validate both issues exist
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        if not self.validate_issue_exists(blocked_by_id):
            raise IssueNotFoundError(blocked_by_id)

        # Check for cycle
        graph = self._get_graph()
        if graph.detect_cycle(issue_id, blocked_by_id):
            raise DependencyError(
                f"Cannot add dependency: {issue_id} -> {blocked_by_id} would create a cycle"
            )

        # Add dependency if not already present
        if blocked_by_id not in issue.blocked_by:
            issue.blocked_by.append(blocked_by_id)
            self.update_issue(issue)

    def remove_dependency(self, issue_id: str, blocked_by_id: str) -> None:
        """
        Remove dependency: issue_id is no longer blocked by blocked_by_id.

        Raises:
            IssueNotFoundError: If the issue doesn't exist.
        """
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        if blocked_by_id in issue.blocked_by:
            issue.blocked_by.remove(blocked_by_id)
            self.update_issue(issue)

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
