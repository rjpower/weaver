"""Tests for weaver.storage."""

from datetime import datetime
from pathlib import Path

import pytest
import yaml

from weaver.models import Issue, Status
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


class TestMarkdownStorage:
    def test_ensure_initialized_creates_directories(self, tmp_path: Path):
        root = tmp_path / ".weaver"
        storage = MarkdownStorage(root)
        storage.ensure_initialized()

        assert (root / "issues").is_dir()
        assert (root / "index.yml").exists()

    def test_issue_path(self, storage: MarkdownStorage, weaver_root: Path):
        path = storage.issue_path("wv-1234")
        assert path == weaver_root / "issues" / "wv-1234.md"

    def test_write_and_read_issue_roundtrip(self, storage: MarkdownStorage):
        issue = Issue(
            id="wv-test",
            title="Test Issue",
            status=Status.OPEN,
            priority=1,
            description="This is a test description.",
            design_notes="Use pytest for testing.",
            acceptance_criteria=["Tests pass", "Coverage > 80%"],
            labels=["backend", "testing"],
            blocked_by=["wv-other"],
            parent="wv-epic1",
            created_at=datetime(2025, 1, 6, 10, 0, 0),
            updated_at=datetime(2025, 1, 6, 12, 0, 0),
        )

        storage.write_issue(issue)
        loaded = storage.read_issue("wv-test")

        assert loaded is not None
        assert loaded.id == issue.id
        assert loaded.title == issue.title
        assert loaded.status == issue.status
        assert loaded.priority == issue.priority
        assert loaded.description == issue.description
        assert loaded.design_notes == issue.design_notes
        assert loaded.acceptance_criteria == issue.acceptance_criteria
        assert loaded.labels == issue.labels
        assert loaded.blocked_by == issue.blocked_by
        assert loaded.parent == issue.parent
        assert loaded.created_at == issue.created_at
        assert loaded.updated_at == issue.updated_at

    def test_read_nonexistent_issue_returns_none(self, storage: MarkdownStorage):
        assert storage.read_issue("wv-nonexistent") is None

    def test_delete_issue(self, storage: MarkdownStorage):
        issue = Issue(id="wv-del", title="To delete")
        storage.write_issue(issue)
        assert storage.read_issue("wv-del") is not None

        result = storage.delete_issue("wv-del")
        assert result is True
        assert storage.read_issue("wv-del") is None

    def test_delete_nonexistent_returns_false(self, storage: MarkdownStorage):
        result = storage.delete_issue("wv-nope")
        assert result is False

    def test_list_issue_ids(self, storage: MarkdownStorage):
        storage.write_issue(Issue(id="wv-001", title="First"))
        storage.write_issue(Issue(id="wv-002", title="Second"))
        storage.write_issue(Issue(id="wv-003", title="Third"))

        ids = storage.list_issue_ids()
        assert set(ids) == {"wv-001", "wv-002", "wv-003"}

    def test_read_all_issues(self, storage: MarkdownStorage):
        storage.write_issue(Issue(id="wv-a", title="Issue A", priority=1))
        storage.write_issue(Issue(id="wv-b", title="Issue B", priority=2))

        issues = storage.read_all_issues()
        assert len(issues) == 2
        titles = {i.title for i in issues}
        assert titles == {"Issue A", "Issue B"}


class TestIndex:
    def test_write_updates_index(self, storage: MarkdownStorage, weaver_root: Path):
        issue = Issue(
            id="wv-idx",
            title="Indexed Issue",
            status=Status.IN_PROGRESS,
            priority=0,
            labels=["urgent"],
            blocked_by=["wv-dep"],
        )
        storage.write_issue(issue)

        with open(weaver_root / "index.yml") as f:
            index = yaml.safe_load(f)

        assert "wv-idx" in index["issues"]
        entry = index["issues"]["wv-idx"]
        assert entry["title"] == "Indexed Issue"
        assert entry["status"] == "in_progress"
        assert entry["priority"] == 0
        assert entry["labels"] == ["urgent"]
        assert entry["blocked_by"] == ["wv-dep"]

    def test_delete_removes_from_index(self, storage: MarkdownStorage, weaver_root: Path):
        issue = Issue(id="wv-gone", title="Will be deleted")
        storage.write_issue(issue)

        with open(weaver_root / "index.yml") as f:
            index = yaml.safe_load(f)
        assert "wv-gone" in index["issues"]

        storage.delete_issue("wv-gone")

        with open(weaver_root / "index.yml") as f:
            index = yaml.safe_load(f)
        assert "wv-gone" not in index["issues"]


class TestMarkdownFormat:
    def test_file_has_yaml_frontmatter(self, storage: MarkdownStorage, weaver_root: Path):
        issue = Issue(id="wv-fmt", title="Format Test")
        storage.write_issue(issue)

        content = (weaver_root / "issues" / "wv-fmt.md").read_text()
        assert content.startswith("---\n")
        # Frontmatter closes with --- (may or may not have trailing content)
        assert "\n---" in content

    def test_preserves_multiline_description(self, storage: MarkdownStorage):
        issue = Issue(
            id="wv-ml",
            title="Multiline",
            description="Line 1\n\nLine 2\n\nLine 3",
        )
        storage.write_issue(issue)
        loaded = storage.read_issue("wv-ml")

        assert loaded is not None
        assert loaded.description == "Line 1\n\nLine 2\n\nLine 3"

    def test_empty_lists_handled(self, storage: MarkdownStorage):
        issue = Issue(
            id="wv-empty",
            title="Empty lists",
            labels=[],
            blocked_by=[],
            acceptance_criteria=[],
        )
        storage.write_issue(issue)
        loaded = storage.read_issue("wv-empty")

        assert loaded is not None
        assert loaded.labels == []
        assert loaded.blocked_by == []
        assert loaded.acceptance_criteria == []

    def test_closed_at_preserved(self, storage: MarkdownStorage):
        closed_time = datetime(2025, 1, 6, 15, 30, 0)
        issue = Issue(
            id="wv-closed",
            title="Closed issue",
            status=Status.CLOSED,
            closed_at=closed_time,
        )
        storage.write_issue(issue)
        loaded = storage.read_issue("wv-closed")

        assert loaded is not None
        assert loaded.closed_at == closed_time
