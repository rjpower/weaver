"""Tests for weaver.service."""

from pathlib import Path

import pytest

from weaver.models import Issue, IssueType, Status
from weaver.service import DependencyError, IssueNotFoundError, IssueService
from weaver.storage import MarkdownStorage


@pytest.fixture
def weaver_root(tmp_path: Path) -> Path:
    root = tmp_path / ".weaver"
    root.mkdir()
    (root / "issues").mkdir()
    return root


@pytest.fixture
def storage(weaver_root: Path) -> MarkdownStorage:
    storage = MarkdownStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def service(storage: MarkdownStorage) -> IssueService:
    return IssueService(storage)


class TestCreateIssue:
    def test_creates_with_defaults(self, service: IssueService):
        issue = service.create_issue("Test issue")

        assert issue.id.startswith("wv-")
        assert issue.title == "Test issue"
        assert issue.status == Status.OPEN
        assert issue.type == IssueType.TASK
        assert issue.priority == 2

    def test_creates_with_all_options(self, service: IssueService):
        issue = service.create_issue(
            title="Feature issue",
            type=IssueType.FEATURE,
            priority=0,
            description="A new feature",
            labels=["backend", "api"],
        )

        assert issue.type == IssueType.FEATURE
        assert issue.priority == 0
        assert issue.description == "A new feature"
        assert issue.labels == ["backend", "api"]

    def test_persists_issue(self, service: IssueService, storage: MarkdownStorage):
        issue = service.create_issue("Persisted")

        loaded = storage.read_issue(issue.id)
        assert loaded is not None
        assert loaded.title == "Persisted"

    def test_rejects_invalid_blocked_by(self, service: IssueService):
        with pytest.raises(DependencyError, match="non-existent"):
            service.create_issue("Test", blocked_by=["wv-nonexistent"])

    def test_accepts_valid_blocked_by(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked", blocked_by=[blocker.id])

        assert blocked.blocked_by == [blocker.id]

    def test_rejects_invalid_parent(self, service: IssueService):
        with pytest.raises(DependencyError, match="Parent"):
            service.create_issue("Test", parent="wv-nonexistent")


class TestGetIssue:
    def test_returns_existing(self, service: IssueService):
        created = service.create_issue("Test")
        found = service.get_issue(created.id)

        assert found is not None
        assert found.id == created.id

    def test_returns_none_for_missing(self, service: IssueService):
        assert service.get_issue("wv-nonexistent") is None


class TestCloseIssue:
    def test_closes_issue(self, service: IssueService):
        issue = service.create_issue("To close")
        closed = service.close_issue(issue.id)

        assert closed.status == Status.CLOSED
        assert closed.closed_at is not None

    def test_raises_for_missing(self, service: IssueService):
        with pytest.raises(IssueNotFoundError):
            service.close_issue("wv-nonexistent")


class TestStartIssue:
    def test_starts_issue(self, service: IssueService):
        issue = service.create_issue("To start")
        started = service.start_issue(issue.id)

        assert started.status == Status.IN_PROGRESS

    def test_raises_for_missing(self, service: IssueService):
        with pytest.raises(IssueNotFoundError):
            service.start_issue("wv-nonexistent")


class TestAddDependency:
    def test_adds_dependency(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked")

        service.add_dependency(blocked.id, blocker.id)

        updated = service.get_issue(blocked.id)
        assert updated is not None
        assert blocker.id in updated.blocked_by

    def test_raises_for_missing_issue(self, service: IssueService):
        blocker = service.create_issue("Blocker")

        with pytest.raises(IssueNotFoundError):
            service.add_dependency("wv-nonexistent", blocker.id)

    def test_raises_for_missing_blocker(self, service: IssueService):
        issue = service.create_issue("Issue")

        with pytest.raises(IssueNotFoundError):
            service.add_dependency(issue.id, "wv-nonexistent")

    def test_rejects_cycle(self, service: IssueService):
        a = service.create_issue("A")
        b = service.create_issue("B")

        service.add_dependency(b.id, a.id)  # B blocked by A

        with pytest.raises(DependencyError, match="cycle"):
            service.add_dependency(a.id, b.id)  # A blocked by B would create cycle

    def test_idempotent(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked")

        service.add_dependency(blocked.id, blocker.id)
        service.add_dependency(blocked.id, blocker.id)  # Second call

        updated = service.get_issue(blocked.id)
        assert updated is not None
        assert updated.blocked_by.count(blocker.id) == 1


class TestRemoveDependency:
    def test_removes_dependency(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked", blocked_by=[blocker.id])

        service.remove_dependency(blocked.id, blocker.id)

        updated = service.get_issue(blocked.id)
        assert updated is not None
        assert blocker.id not in updated.blocked_by

    def test_raises_for_missing_issue(self, service: IssueService):
        with pytest.raises(IssueNotFoundError):
            service.remove_dependency("wv-nonexistent", "wv-other")

    def test_no_error_for_missing_dep(self, service: IssueService):
        issue = service.create_issue("Issue")
        # Should not raise even if dependency doesn't exist
        service.remove_dependency(issue.id, "wv-other")


class TestListIssues:
    def test_returns_all_by_default(self, service: IssueService):
        service.create_issue("Issue 1")
        service.create_issue("Issue 2")

        issues = service.list_issues()
        assert len(issues) == 2

    def test_filters_by_status(self, service: IssueService):
        issue = service.create_issue("Open")
        service.create_issue("Closed")
        service.close_issue(service.list_issues()[1].id)

        open_issues = service.list_issues(status=Status.OPEN)
        assert len(open_issues) == 1
        assert open_issues[0].status == Status.OPEN

    def test_filters_by_labels(self, service: IssueService):
        service.create_issue("Backend", labels=["backend"])
        service.create_issue("Frontend", labels=["frontend"])
        service.create_issue("Full stack", labels=["backend", "frontend"])

        backend = service.list_issues(labels=["backend"])
        assert len(backend) == 2

    def test_filters_by_type(self, service: IssueService):
        service.create_issue("Task", type=IssueType.TASK)
        service.create_issue("Bug", type=IssueType.BUG)

        bugs = service.list_issues(type=IssueType.BUG)
        assert len(bugs) == 1
        assert bugs[0].type == IssueType.BUG

    def test_sorted_by_priority_then_date(self, service: IssueService):
        p2 = service.create_issue("Priority 2", priority=2)
        p0 = service.create_issue("Priority 0", priority=0)
        p1 = service.create_issue("Priority 1", priority=1)

        issues = service.list_issues()
        priorities = [i.priority for i in issues]
        assert priorities == [0, 1, 2]


class TestGetReadyIssues:
    def test_excludes_blocked(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        service.create_issue("Blocked", blocked_by=[blocker.id])

        ready = service.get_ready_issues()
        ids = {i.id for i in ready}
        assert blocker.id in ids
        assert len(ready) == 1

    def test_includes_when_blocker_closed(self, service: IssueService):
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked", blocked_by=[blocker.id])

        service.close_issue(blocker.id)

        ready = service.get_ready_issues()
        ids = {i.id for i in ready}
        assert blocked.id in ids

    def test_filters_by_labels(self, service: IssueService):
        service.create_issue("Backend", labels=["backend"])
        service.create_issue("Frontend", labels=["frontend"])

        ready = service.get_ready_issues(labels=["backend"])
        assert len(ready) == 1
        assert ready[0].labels == ["backend"]

    def test_filters_by_type(self, service: IssueService):
        service.create_issue("Task", type=IssueType.TASK)
        service.create_issue("Bug", type=IssueType.BUG)

        ready = service.get_ready_issues(type=IssueType.BUG)
        assert len(ready) == 1
        assert ready[0].type == IssueType.BUG

    def test_respects_limit(self, service: IssueService):
        for i in range(5):
            service.create_issue(f"Issue {i}")

        ready = service.get_ready_issues(limit=3)
        assert len(ready) == 3

    def test_excludes_closed(self, service: IssueService):
        open_issue = service.create_issue("Open")
        closed_issue = service.create_issue("Closed")
        service.close_issue(closed_issue.id)

        ready = service.get_ready_issues()
        ids = {i.id for i in ready}
        assert open_issue.id in ids
        assert closed_issue.id not in ids
