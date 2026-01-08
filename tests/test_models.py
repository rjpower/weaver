"""Tests for weaver.models."""

import re

import pytest

from weaver.models import Hint, Issue, Status, Workflow, WorkflowStep, generate_id


class TestStatus:
    def test_enum_values(self):
        assert Status.OPEN.value == "open"
        assert Status.IN_PROGRESS.value == "in_progress"
        assert Status.BLOCKED.value == "blocked"
        assert Status.CLOSED.value == "closed"

    def test_from_string(self):
        assert Status("open") == Status.OPEN
        assert Status("in_progress") == Status.IN_PROGRESS


class TestIssue:
    def test_defaults(self):
        issue = Issue(id="wv-1234", title="Test issue")
        assert issue.status == Status.OPEN
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


class TestHint:
    def test_defaults(self):
        hint = Hint(id="wv-5678", title="Test hint", content="Test breadcrumb")
        assert hint.labels == []
        assert hint.created_at is not None
        assert hint.updated_at is not None

    def test_required_fields(self):
        hint = Hint(id="wv-5678", title="Test hint", content="Test breadcrumb")
        assert hint.id == "wv-5678"
        assert hint.title == "Test hint"
        assert hint.content == "Test breadcrumb"

    def test_labels(self):
        hint = Hint(
            id="wv-5678",
            title="Test hint",
            content="Test breadcrumb",
            labels=["python", "testing"],
        )
        assert hint.labels == ["python", "testing"]


class TestWorkflowStep:
    def test_defaults(self):
        step = WorkflowStep(title="Test step")
        assert step.priority == 2
        assert step.description == ""
        assert step.labels == []
        assert step.depends_on == []

    def test_required_fields(self):
        step = WorkflowStep(title="Test step")
        assert step.title == "Test step"

    def test_custom_values(self):
        step = WorkflowStep(
            title="Design phase",
            priority=1,
            description="Design the feature",
            labels=["design", "phase1"],
            depends_on=["Requirements"],
        )
        assert step.title == "Design phase"
        assert step.priority == 1
        assert step.description == "Design the feature"
        assert step.labels == ["design", "phase1"]
        assert step.depends_on == ["Requirements"]


class TestWorkflow:
    def test_defaults(self):
        workflow = Workflow(id="wf-1", name="design")
        assert workflow.description == ""
        assert workflow.steps == []
        assert workflow.created_at is not None
        assert workflow.updated_at is not None

    def test_required_fields(self):
        workflow = Workflow(id="wf-1", name="design")
        assert workflow.id == "wf-1"
        assert workflow.name == "design"

    def test_with_steps(self):
        steps = [
            WorkflowStep(title="Step 1"),
            WorkflowStep(title="Step 2", depends_on=["Step 1"]),
        ]
        workflow = Workflow(
            id="wf-1",
            name="feature-dev",
            description="Feature development workflow",
            steps=steps,
        )
        assert workflow.name == "feature-dev"
        assert workflow.description == "Feature development workflow"
        assert len(workflow.steps) == 2
        assert workflow.steps[0].title == "Step 1"
        assert workflow.steps[1].title == "Step 2"
        assert workflow.steps[1].depends_on == ["Step 1"]


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
