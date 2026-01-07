"""Tests for WorkflowService."""

from pathlib import Path

import pytest
import yaml

from weaver.models import IssueType, Status
from weaver.service import IssueService, WorkflowService
from weaver.storage import MarkdownStorage, WorkflowStorage


@pytest.fixture
def weaver_root(tmp_path: Path) -> Path:
    root = tmp_path / ".weaver"
    root.mkdir()
    return root


@pytest.fixture
def issue_storage(weaver_root: Path) -> MarkdownStorage:
    storage = MarkdownStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def workflow_storage(weaver_root: Path) -> WorkflowStorage:
    storage = WorkflowStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def issue_service(issue_storage: MarkdownStorage) -> IssueService:
    return IssueService(issue_storage)


@pytest.fixture
def workflow_service(
    workflow_storage: WorkflowStorage, issue_service: IssueService
) -> WorkflowService:
    return WorkflowService(workflow_storage, issue_service)


def simple_workflow_yaml() -> str:
    return """
name: simple-workflow
description: A simple workflow
steps:
  - title: Step 1
    type: task
    priority: 1
    description: First step
    labels:
      - backend
    depends_on: []
  - title: Step 2
    type: feature
    priority: 2
    description: Second step
    labels:
      - frontend
    depends_on:
      - Step 1
"""


def complex_workflow_yaml() -> str:
    return """
name: complex-workflow
description: A complex workflow with multiple dependencies
steps:
  - title: Foundation
    type: task
    priority: 0
    description: Base work
    labels:
      - core
    depends_on: []
  - title: Backend API
    type: feature
    priority: 1
    description: API implementation
    labels:
      - backend
    depends_on:
      - Foundation
  - title: Frontend UI
    type: feature
    priority: 1
    description: UI implementation
    labels:
      - frontend
    depends_on:
      - Foundation
  - title: Integration
    type: task
    priority: 2
    description: Integrate frontend and backend
    labels:
      - integration
    depends_on:
      - Backend API
      - Frontend UI
"""


def minimal_workflow_yaml() -> str:
    return """
name: minimal
steps:
  - title: Only Step
"""


class TestParseWorkflowYaml:
    def test_parses_simple_workflow(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.parse_workflow_yaml(yaml_content)

        assert workflow.id.startswith("wv-workflow-")
        assert workflow.name == "simple-workflow"
        assert workflow.description == "A simple workflow"
        assert len(workflow.steps) == 2

    def test_parses_step_properties(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.parse_workflow_yaml(yaml_content)

        step1 = workflow.steps[0]
        assert step1.title == "Step 1"
        assert step1.type == IssueType.TASK
        assert step1.priority == 1
        assert step1.description == "First step"
        assert step1.labels == ["backend"]
        assert step1.depends_on == []

        step2 = workflow.steps[1]
        assert step2.title == "Step 2"
        assert step2.type == IssueType.FEATURE
        assert step2.priority == 2
        assert step2.description == "Second step"
        assert step2.labels == ["frontend"]
        assert step2.depends_on == ["Step 1"]

    def test_parses_minimal_workflow(self, workflow_service: WorkflowService):
        yaml_content = minimal_workflow_yaml()
        workflow = workflow_service.parse_workflow_yaml(yaml_content)

        assert workflow.name == "minimal"
        assert workflow.description == ""
        assert len(workflow.steps) == 1

        step = workflow.steps[0]
        assert step.title == "Only Step"
        assert step.type == IssueType.TASK  # default
        assert step.priority == 2  # default
        assert step.description == ""
        assert step.labels == []
        assert step.depends_on == []

    def test_parses_complex_workflow(self, workflow_service: WorkflowService):
        yaml_content = complex_workflow_yaml()
        workflow = workflow_service.parse_workflow_yaml(yaml_content)

        assert workflow.name == "complex-workflow"
        assert len(workflow.steps) == 4

        integration_step = workflow.steps[3]
        assert integration_step.title == "Integration"
        assert integration_step.depends_on == ["Backend API", "Frontend UI"]


class TestCreateOrUpdateWorkflow:
    def test_creates_new_workflow(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        assert workflow.id.startswith("wv-workflow-")
        assert workflow.name == "simple-workflow"

        # Verify it was persisted
        loaded = workflow_service.storage.read_workflow(workflow.id)
        assert loaded is not None
        assert loaded.name == "simple-workflow"

    def test_updates_existing_workflow_preserves_id(
        self, workflow_service: WorkflowService
    ):
        yaml_content = simple_workflow_yaml()
        original = workflow_service.create_or_update_workflow(yaml_content)
        original_id = original.id
        original_created_at = original.created_at

        # Update with modified YAML
        updated_yaml = """
name: simple-workflow
description: Updated description
steps:
  - title: New Step
    type: bug
"""
        updated = workflow_service.create_or_update_workflow(updated_yaml)

        # ID and created_at should be preserved
        assert updated.id == original_id
        assert updated.created_at == original_created_at
        assert updated.updated_at > original.created_at

        # Description and steps should be updated
        assert updated.description == "Updated description"
        assert len(updated.steps) == 1
        assert updated.steps[0].title == "New Step"

    def test_update_is_case_insensitive(self, workflow_service: WorkflowService):
        yaml1 = """
name: MyWorkflow
steps:
  - title: Step 1
"""
        yaml2 = """
name: myworkflow
steps:
  - title: Step 2
"""
        original = workflow_service.create_or_update_workflow(yaml1)
        updated = workflow_service.create_or_update_workflow(yaml2)

        # Should update the same workflow
        assert updated.id == original.id


class TestExecuteWorkflow:
    def test_creates_issues_from_workflow(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        assert len(issues) == 2
        assert issues[0].title == "Step 1"
        assert issues[1].title == "Step 2"

    def test_applies_workflow_label(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        for issue in issues:
            assert "workflow:simple-workflow" in issue.labels

    def test_applies_custom_label_prefix(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name, label_prefix="v1.0")

        for issue in issues:
            assert "workflow:v1.0" in issue.labels

    def test_preserves_step_labels(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        step1_issue = next(i for i in issues if i.title == "Step 1")
        assert "backend" in step1_issue.labels
        assert "workflow:simple-workflow" in step1_issue.labels

        step2_issue = next(i for i in issues if i.title == "Step 2")
        assert "frontend" in step2_issue.labels
        assert "workflow:simple-workflow" in step2_issue.labels

    def test_creates_dependencies_correctly(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        step1_issue = next(i for i in issues if i.title == "Step 1")
        step2_issue = next(i for i in issues if i.title == "Step 2")

        # Step 1 should have no dependencies
        assert step1_issue.blocked_by == []

        # Step 2 should be blocked by Step 1
        assert len(step2_issue.blocked_by) == 1
        assert step2_issue.blocked_by[0] == step1_issue.id

    def test_creates_complex_dependencies(self, workflow_service: WorkflowService):
        yaml_content = complex_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        # Find issues by title
        issues_by_title = {issue.title: issue for issue in issues}

        foundation = issues_by_title["Foundation"]
        backend = issues_by_title["Backend API"]
        frontend = issues_by_title["Frontend UI"]
        integration = issues_by_title["Integration"]

        # Foundation has no dependencies
        assert foundation.blocked_by == []

        # Backend and Frontend depend on Foundation
        assert backend.blocked_by == [foundation.id]
        assert frontend.blocked_by == [foundation.id]

        # Integration depends on both Backend and Frontend
        assert set(integration.blocked_by) == {backend.id, frontend.id}

    def test_preserves_issue_properties(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.name)

        step1_issue = next(i for i in issues if i.title == "Step 1")
        assert step1_issue.type == IssueType.TASK
        assert step1_issue.priority == 1
        assert step1_issue.description == "First step"
        assert step1_issue.status == Status.OPEN

        step2_issue = next(i for i in issues if i.title == "Step 2")
        assert step2_issue.type == IssueType.FEATURE
        assert step2_issue.priority == 2
        assert step2_issue.description == "Second step"

    def test_executes_by_workflow_id(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        issues = workflow_service.execute_workflow(workflow.id)

        assert len(issues) == 2
        assert issues[0].title == "Step 1"

    def test_raises_for_missing_workflow(self, workflow_service: WorkflowService):
        with pytest.raises(ValueError, match="Workflow not found"):
            workflow_service.execute_workflow("nonexistent")

    def test_validates_invalid_dependencies(self, workflow_service: WorkflowService):
        yaml_content = """
name: invalid-deps
steps:
  - title: Step 1
    depends_on:
      - Nonexistent Step
"""
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        with pytest.raises(ValueError, match="Invalid dependency.*Nonexistent Step"):
            workflow_service.execute_workflow(workflow.name)

    def test_validates_missing_dependency_in_multi_step(
        self, workflow_service: WorkflowService
    ):
        yaml_content = """
name: invalid-multi
steps:
  - title: Step 1
  - title: Step 2
    depends_on:
      - Missing Step
"""
        workflow = workflow_service.create_or_update_workflow(yaml_content)

        with pytest.raises(ValueError, match="Invalid dependency.*Missing Step"):
            workflow_service.execute_workflow(workflow.name)


class TestGetWorkflow:
    def test_gets_by_id(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        created = workflow_service.create_or_update_workflow(yaml_content)

        found = workflow_service.get_workflow(created.id)

        assert found is not None
        assert found.id == created.id
        assert found.name == "simple-workflow"

    def test_gets_by_name(self, workflow_service: WorkflowService):
        yaml_content = simple_workflow_yaml()
        created = workflow_service.create_or_update_workflow(yaml_content)

        found = workflow_service.get_workflow("simple-workflow")

        assert found is not None
        assert found.id == created.id
        assert found.name == "simple-workflow"

    def test_gets_by_name_case_insensitive(
        self, workflow_service: WorkflowService
    ):
        yaml_content = simple_workflow_yaml()
        workflow_service.create_or_update_workflow(yaml_content)

        found = workflow_service.get_workflow("SIMPLE-WORKFLOW")

        assert found is not None
        assert found.name == "simple-workflow"

    def test_returns_none_for_missing(self, workflow_service: WorkflowService):
        result = workflow_service.get_workflow("nonexistent")
        assert result is None


class TestListWorkflows:
    def test_returns_empty_list_initially(self, workflow_service: WorkflowService):
        workflows = workflow_service.list_workflows()
        assert workflows == []

    def test_returns_all_workflows(self, workflow_service: WorkflowService):
        workflow_service.create_or_update_workflow(simple_workflow_yaml())
        workflow_service.create_or_update_workflow(complex_workflow_yaml())

        workflows = workflow_service.list_workflows()

        assert len(workflows) == 2
        names = {w.name for w in workflows}
        assert names == {"simple-workflow", "complex-workflow"}

    def test_workflows_sorted_by_name(self, workflow_service: WorkflowService):
        workflow_service.create_or_update_workflow(
            """
name: zebra
steps:
  - title: Step
"""
        )
        workflow_service.create_or_update_workflow(
            """
name: alpha
steps:
  - title: Step
"""
        )

        workflows = workflow_service.list_workflows()

        assert workflows[0].name == "alpha"
        assert workflows[1].name == "zebra"
