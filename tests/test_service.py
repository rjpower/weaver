"""Tests for weaver.service."""

from pathlib import Path

import pytest

from weaver.models import Hint, Issue, IssueType, Status
from weaver.service import DependencyError, IssueNotFoundError, IssueService, HintService
from weaver.storage import HintStorage, MarkdownStorage


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


class TestGetIssueWithDependencies:
    def test_issue_with_no_dependencies(self, service: IssueService):
        issue = service.create_issue("No deps")

        main_issue, dependencies = service.get_issue_with_dependencies(issue.id)

        assert main_issue.id == issue.id
        assert dependencies == []

    def test_issue_with_single_level_dependencies(self, service: IssueService):
        dep1 = service.create_issue("Dep 1")
        dep2 = service.create_issue("Dep 2")
        main = service.create_issue("Main", blocked_by=[dep1.id, dep2.id])

        main_issue, dependencies = service.get_issue_with_dependencies(main.id)

        assert main_issue.id == main.id
        assert len(dependencies) == 2
        dep_ids = {d.id for d in dependencies}
        assert dep1.id in dep_ids
        assert dep2.id in dep_ids

    def test_issue_with_multi_level_dependencies(self, service: IssueService):
        # Create chain: main -> dep1 -> dep2 -> dep3
        dep3 = service.create_issue("Dep 3")
        dep2 = service.create_issue("Dep 2", blocked_by=[dep3.id])
        dep1 = service.create_issue("Dep 1", blocked_by=[dep2.id])
        main = service.create_issue("Main", blocked_by=[dep1.id])

        main_issue, dependencies = service.get_issue_with_dependencies(main.id)

        assert main_issue.id == main.id
        assert len(dependencies) == 3

        # Dependencies should be in topological order (deepest first)
        dep_ids = [d.id for d in dependencies]
        assert dep3.id in dep_ids
        assert dep2.id in dep_ids
        assert dep1.id in dep_ids

        # Verify topological ordering: dep3 before dep2, dep2 before dep1
        dep3_idx = dep_ids.index(dep3.id)
        dep2_idx = dep_ids.index(dep2.id)
        dep1_idx = dep_ids.index(dep1.id)
        assert dep3_idx < dep2_idx
        assert dep2_idx < dep1_idx

    def test_missing_issue_raises_error(self, service: IssueService):
        with pytest.raises(IssueNotFoundError, match="wv-nonexistent"):
            service.get_issue_with_dependencies("wv-nonexistent")


@pytest.fixture
def hint_storage(weaver_root: Path) -> HintStorage:
    storage = HintStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def hint_service(hint_storage: HintStorage) -> HintService:
    return HintService(hint_storage)


class TestCreateOrUpdateHint:
    def test_creates_new_hint(self, hint_service: HintService):
        hint = hint_service.create_or_update_hint(
            title="Test Hint", content="This is a test hint"
        )

        assert hint.id.startswith("wv-hint-")
        assert hint.title == "test hint"
        assert hint.content == "This is a test hint"
        assert hint.labels == []

    def test_creates_with_labels(self, hint_service: HintService):
        hint = hint_service.create_or_update_hint(
            title="Test Hint", content="Content", labels=["python", "testing"]
        )

        assert hint.labels == ["python", "testing"]

    def test_normalizes_title_to_lowercase(self, hint_service: HintService):
        hint = hint_service.create_or_update_hint(
            title="Test HINT Title", content="Content"
        )

        assert hint.title == "test hint title"

    def test_updates_existing_hint_by_title(self, hint_service: HintService):
        original = hint_service.create_or_update_hint(
            title="Test Hint", content="Original content", labels=["old"]
        )
        original_id = original.id

        updated = hint_service.create_or_update_hint(
            title="Test Hint", content="Updated content", labels=["new"]
        )

        assert updated.id == original_id
        assert updated.content == "Updated content"
        assert updated.labels == ["new"]

    def test_updates_case_insensitive(self, hint_service: HintService):
        original = hint_service.create_or_update_hint(
            title="test hint", content="Original"
        )
        original_id = original.id

        updated = hint_service.create_or_update_hint(
            title="TEST HINT", content="Updated"
        )

        assert updated.id == original_id
        assert updated.content == "Updated"

    def test_persists_hint(self, hint_service: HintService, hint_storage: HintStorage):
        hint = hint_service.create_or_update_hint(title="Persisted", content="Content")

        loaded = hint_storage.read_hint(hint.id)
        assert loaded is not None
        assert loaded.title == "persisted"
        assert loaded.content == "Content"


class TestGetHint:
    def test_gets_by_id(self, hint_service: HintService):
        created = hint_service.create_or_update_hint(
            title="Test Hint", content="Content"
        )

        found = hint_service.get_hint(created.id)
        assert found is not None
        assert found.id == created.id

    def test_gets_by_title_exact(self, hint_service: HintService):
        created = hint_service.create_or_update_hint(
            title="Test Hint", content="Content"
        )

        found = hint_service.get_hint("test hint")
        assert found is not None
        assert found.id == created.id

    def test_gets_by_title_case_insensitive(self, hint_service: HintService):
        created = hint_service.create_or_update_hint(
            title="Test Hint", content="Content"
        )

        found = hint_service.get_hint("TEST HINT")
        assert found is not None
        assert found.id == created.id

    def test_returns_none_for_missing(self, hint_service: HintService):
        assert hint_service.get_hint("wv-hint-nonexistent") is None
        assert hint_service.get_hint("nonexistent title") is None

    def test_id_lookup_takes_precedence(
        self, hint_service: HintService, hint_storage: HintStorage
    ):
        # Create a hint with a title that could be confused with an ID
        hint1 = hint_service.create_or_update_hint(
            title="Regular Hint", content="Content 1"
        )

        # Manually create another hint with a title that matches hint1's ID
        hint2 = Hint(
            id="wv-hint-test2",
            title=hint1.id,
            content="Content 2",
        )
        hint_storage.write_hint(hint2)

        # Searching by hint1's ID should return hint1, not hint2
        found = hint_service.get_hint(hint1.id)
        assert found is not None
        assert found.id == hint1.id
        assert found.content == "Content 1"


class TestListHints:
    def test_returns_empty_list_when_no_hints(self, hint_service: HintService):
        hints = hint_service.list_hints()
        assert hints == []

    def test_returns_all_hints(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Hint 1", content="Content 1")
        hint_service.create_or_update_hint(title="Hint 2", content="Content 2")

        hints = hint_service.list_hints()
        assert len(hints) == 2

    def test_sorted_by_title(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Zebra", content="Content")
        hint_service.create_or_update_hint(title="Apple", content="Content")
        hint_service.create_or_update_hint(title="Mango", content="Content")

        hints = hint_service.list_hints()
        titles = [h.title for h in hints]
        assert titles == ["apple", "mango", "zebra"]


class TestSearchHints:
    def test_finds_by_title(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Python Testing", content="Content")
        hint_service.create_or_update_hint(title="Java Tutorial", content="Content")

        results = hint_service.search_hints("python")
        assert len(results) == 1
        assert results[0].title == "python testing"

    def test_finds_by_content(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Hint 1", content="Use pytest")
        hint_service.create_or_update_hint(title="Hint 2", content="Use unittest")

        results = hint_service.search_hints("pytest")
        assert len(results) == 1
        assert results[0].title == "hint 1"

    def test_case_insensitive_search(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Python", content="Content")

        results = hint_service.search_hints("PYTHON")
        assert len(results) == 1

    def test_returns_multiple_matches(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Python Testing", content="Content")
        hint_service.create_or_update_hint(title="Testing Guide", content="Python")
        hint_service.create_or_update_hint(title="Java", content="Content")

        results = hint_service.search_hints("python")
        assert len(results) == 2

    def test_returns_empty_for_no_matches(self, hint_service: HintService):
        hint_service.create_or_update_hint(title="Python", content="Content")

        results = hint_service.search_hints("javascript")
        assert results == []
