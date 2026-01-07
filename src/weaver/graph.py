"""Dependency graph for Weaver issue tracker."""

from collections import defaultdict
from dataclasses import dataclass

from weaver.models import Issue, Status


@dataclass
class DependencyGraph:
    """DAG of issue dependencies for computing ready queue."""

    blocked_by: dict[str, set[str]]  # issue_id -> IDs blocking it
    blocks: dict[str, set[str]]  # issue_id -> IDs it blocks
    all_ids: set[str]

    @classmethod
    def build(cls, issues: list[Issue]) -> "DependencyGraph":
        """Build dependency graph from a list of issues."""
        blocked_by: dict[str, set[str]] = defaultdict(set)
        blocks: dict[str, set[str]] = defaultdict(set)
        all_ids: set[str] = set()

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
            issue
            for issue in open_issues
            if issue.is_open()
            and issue.status != Status.BLOCKED
            and not self.is_blocked(issue.id, open_ids)
        ]

    def detect_cycle(self, from_id: str, to_id: str) -> bool:
        """
        Check if adding from_id -> to_id dependency creates a cycle.

        The dependency means from_id is blocked_by to_id.
        A cycle would occur if to_id is already (transitively) blocked by from_id.
        """
        # DFS from to_id following blocked_by edges to see if we reach from_id
        visited: set[str] = set()
        stack = [to_id]

        while stack:
            current = stack.pop()
            if current == from_id:
                return True
            if current in visited:
                continue
            visited.add(current)
            stack.extend(self.blocked_by.get(current, set()))

        return False

    def get_blockers(self, issue_id: str) -> set[str]:
        """Get IDs of issues blocking the given issue."""
        return self.blocked_by.get(issue_id, set()).copy()

    def get_blocked_by_this(self, issue_id: str) -> set[str]:
        """Get IDs of issues blocked by the given issue."""
        return self.blocks.get(issue_id, set()).copy()

    def get_transitive_blockers(self, issue_id: str) -> list[str]:
        """Get all transitive blockers in topological order (deepest first).

        Returns a list of issue IDs where dependencies appear before dependents.
        """
        visited: set[str] = set()
        result: list[str] = []

        def dfs(current_id: str) -> None:
            if current_id in visited:
                return
            visited.add(current_id)

            # Visit all blockers first (depth-first)
            for blocker_id in self.blocked_by.get(current_id, set()):
                dfs(blocker_id)

            # Add current node after all its dependencies
            if current_id != issue_id:  # Don't include the starting issue
                result.append(current_id)

        dfs(issue_id)
        return result
