"""Tests for weaver.models."""

import re

import pytest

from weaver.models import Issue, IssueType, Status, generate_id


class TestStatus:
    def test_enum_values(self):
        assert Status.OPEN.value == "open"
        assert Status.IN_PROGRESS.value == "in_progress"
        assert Status.BLOCKED.value == "blocked"
        assert Status.CLOSED.value == "closed"

    def test_from_string(self):
        assert Status("open") == Status.OPEN
        assert Status("in_progress") == Status.IN_PROGRESS


class TestIssueType:
    def test_enum_values(self):
        assert IssueType.TASK.value == "task"
        assert IssueType.BUG.value == "bug"
        assert IssueType.FEATURE.value == "feature"
        assert IssueType.EPIC.value == "epic"
        assert IssueType.CHORE.value == "chore"


class TestIssue:
    def test_defaults(self):
        issue = Issue(id="wv-1234", title="Test issue")
        assert issue.status == Status.OPEN
        assert issue.type == IssueType.TASK
        assert issue.priority == 2
        assert issue.description == ""
        assert issue.design_notes == ""
        assert issue.acceptance_criteria == []
        assert issue.labels == []
        assert issue.blocked_by == []
        assert issue.parent is None
        assert issue.closed_at is None

    def test_is_open(self):
        issue = Issue(id="wv-1234", title="Test")
        assert issue.is_open() is True

        issue.status = Status.IN_PROGRESS
        assert issue.is_open() is True

        issue.status = Status.BLOCKED
        assert issue.is_open() is True

        issue.status = Status.CLOSED
        assert issue.is_open() is False

    def test_content_hash_deterministic(self):
        issue1 = Issue(id="wv-1", title="Test", description="desc", design_notes="notes")
        issue2 = Issue(id="wv-2", title="Test", description="desc", design_notes="notes")
        assert issue1.content_hash == issue2.content_hash

    def test_content_hash_changes_with_content(self):
        issue1 = Issue(id="wv-1", title="Test A")
        issue2 = Issue(id="wv-1", title="Test B")
        assert issue1.content_hash != issue2.content_hash


class TestGenerateId:
    def test_format(self):
        id = generate_id()
        assert re.match(r"^wv-[a-f0-9]{4}$", id)

    def test_custom_prefix(self):
        id = generate_id(prefix="test")
        assert id.startswith("test-")

    def test_uniqueness(self):
        ids = {generate_id() for _ in range(100)}
        assert len(ids) == 100
