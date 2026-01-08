"""Tests for WorkflowStorage."""

import pytest
from datetime import datetime
from pathlib import Path

from weaver.models import Workflow, WorkflowStep
from weaver.storage import WorkflowStorage


@pytest.fixture
def temp_storage(tmp_path):
    """Create a temporary WorkflowStorage instance."""
    storage = WorkflowStorage(tmp_path / ".weaver")
    storage.ensure_initialized()
    return storage


@pytest.fixture
def sample_workflow():
    """Create a sample workflow for testing."""
    return Workflow(
        id="wv-test-a3f8",
        name="design",
        description="Standard design workflow",
        created_at=datetime(2026, 1, 7, 10, 0, 0),
        updated_at=datetime(2026, 1, 7, 10, 0, 0),
        steps=[
            WorkflowStep(
                title="Background research",
                priority=1,
                description="Research existing solutions",
                labels=["research"],
                depends_on=[],
            ),
            WorkflowStep(
                title="Design document",
                priority=1,
                description="Write design doc",
                depends_on=["Background research"],
            ),
        ],
    )


def test_ensure_initialized_creates_directory(tmp_path):
    """Test that ensure_initialized creates the workflows directory."""
    storage = WorkflowStorage(tmp_path / ".weaver")
    assert not storage.workflows_dir.exists()

    storage.ensure_initialized()

    assert storage.workflows_dir.exists()
    assert storage.workflows_dir.is_dir()


def test_workflow_path(temp_storage):
    """Test that workflow_path returns correct path."""
    path = temp_storage.workflow_path("wv-test-a3f8")

    assert path == temp_storage.workflows_dir / "wv-test-a3f8.yml"
    assert path.suffix == ".yml"


def test_write_workflow(temp_storage, sample_workflow):
    """Test writing a workflow to YAML file."""
    temp_storage.write_workflow(sample_workflow)

    path = temp_storage.workflow_path(sample_workflow.id)
    assert path.exists()

    content = path.read_text()
    assert "id: wv-test-a3f8" in content
    assert "name: design" in content
    assert "description: Standard design workflow" in content
    assert "title: Background research" in content


def test_read_workflow(temp_storage, sample_workflow):
    """Test reading a workflow from YAML file."""
    temp_storage.write_workflow(sample_workflow)

    loaded = temp_storage.read_workflow(sample_workflow.id)

    assert loaded is not None
    assert loaded.id == sample_workflow.id
    assert loaded.name == sample_workflow.name
    assert loaded.description == sample_workflow.description
    assert loaded.created_at == sample_workflow.created_at
    assert loaded.updated_at == sample_workflow.updated_at
    assert len(loaded.steps) == 2


def test_read_workflow_nonexistent(temp_storage):
    """Test reading a workflow that doesn't exist returns None."""
    result = temp_storage.read_workflow("wv-nonexistent")

    assert result is None


def test_workflow_step_serialization(temp_storage, sample_workflow):
    """Test that WorkflowStep fields are properly serialized and deserialized."""
    temp_storage.write_workflow(sample_workflow)
    loaded = temp_storage.read_workflow(sample_workflow.id)

    assert loaded is not None
    step1 = loaded.steps[0]
    assert step1.title == "Background research"
    assert step1.priority == 1
    assert step1.description == "Research existing solutions"
    assert step1.labels == ["research"]
    assert step1.depends_on == []

    step2 = loaded.steps[1]
    assert step2.title == "Design document"
    assert step2.depends_on == ["Background research"]


def test_workflow_datetime_serialization(temp_storage, sample_workflow):
    """Test that datetime fields are properly serialized to ISO format."""
    temp_storage.write_workflow(sample_workflow)

    path = temp_storage.workflow_path(sample_workflow.id)
    content = path.read_text()

    assert "created_at: '2026-01-07T10:00:00'" in content
    assert "updated_at: '2026-01-07T10:00:00'" in content


def test_find_workflow_by_name(temp_storage, sample_workflow):
    """Test finding a workflow by name."""
    temp_storage.write_workflow(sample_workflow)

    found = temp_storage.find_workflow_by_name("design")

    assert found is not None
    assert found.id == sample_workflow.id
    assert found.name == "design"


def test_find_workflow_by_name_case_insensitive(temp_storage, sample_workflow):
    """Test that find_workflow_by_name is case-insensitive."""
    temp_storage.write_workflow(sample_workflow)

    found = temp_storage.find_workflow_by_name("DESIGN")
    assert found is not None

    found = temp_storage.find_workflow_by_name("Design")
    assert found is not None


def test_find_workflow_by_name_not_found(temp_storage):
    """Test that find_workflow_by_name returns None when not found."""
    result = temp_storage.find_workflow_by_name("nonexistent")

    assert result is None


def test_list_all_workflows(temp_storage):
    """Test listing all workflows."""
    workflow1 = Workflow(id="wv-001", name="alpha", description="First workflow")
    workflow2 = Workflow(id="wv-002", name="beta", description="Second workflow")
    workflow3 = Workflow(id="wv-003", name="gamma", description="Third workflow")

    temp_storage.write_workflow(workflow1)
    temp_storage.write_workflow(workflow2)
    temp_storage.write_workflow(workflow3)

    workflows = temp_storage.list_all_workflows()

    assert len(workflows) == 3
    assert workflows[0].name == "alpha"
    assert workflows[1].name == "beta"
    assert workflows[2].name == "gamma"


def test_list_all_workflows_sorted_by_name(temp_storage):
    """Test that list_all_workflows returns workflows sorted by name."""
    workflow1 = Workflow(id="wv-001", name="zebra", description="Last alphabetically")
    workflow2 = Workflow(id="wv-002", name="apple", description="First alphabetically")
    workflow3 = Workflow(id="wv-003", name="banana", description="Middle alphabetically")

    temp_storage.write_workflow(workflow1)
    temp_storage.write_workflow(workflow2)
    temp_storage.write_workflow(workflow3)

    workflows = temp_storage.list_all_workflows()

    assert len(workflows) == 3
    assert workflows[0].name == "apple"
    assert workflows[1].name == "banana"
    assert workflows[2].name == "zebra"


def test_list_all_workflows_empty(temp_storage):
    """Test that list_all_workflows returns empty list when no workflows exist."""
    workflows = temp_storage.list_all_workflows()

    assert workflows == []


def test_list_all_workflows_no_directory(tmp_path):
    """Test that list_all_workflows returns empty list when directory doesn't exist."""
    storage = WorkflowStorage(tmp_path / ".weaver")

    workflows = storage.list_all_workflows()

    assert workflows == []


def test_workflow_with_empty_steps(temp_storage):
    """Test workflow with no steps."""
    workflow = Workflow(
        id="wv-empty",
        name="empty",
        description="Workflow with no steps",
        steps=[],
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-empty")

    assert loaded is not None
    assert loaded.steps == []


def test_workflow_step_with_defaults(temp_storage):
    """Test workflow step with default values."""
    workflow = Workflow(
        id="wv-defaults",
        name="defaults",
        steps=[
            WorkflowStep(title="Simple task"),
        ],
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-defaults")

    assert loaded is not None
    step = loaded.steps[0]
    assert step.title == "Simple task"
    assert step.priority == 2
    assert step.description == ""
    assert step.labels == []
    assert step.depends_on == []


def test_workflow_step_with_multiple_priorities(temp_storage):
    """Test workflow steps with different priorities."""
    workflow = Workflow(
        id="wv-priorities",
        name="priorities",
        steps=[
            WorkflowStep(title="Bug fix", priority=0),
            WorkflowStep(title="New feature", priority=1),
            WorkflowStep(title="Enhancement", priority=2),
            WorkflowStep(title="Nice to have", priority=4),
        ],
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-priorities")

    assert loaded is not None
    assert loaded.steps[0].priority == 0
    assert loaded.steps[1].priority == 1
    assert loaded.steps[2].priority == 2
    assert loaded.steps[3].priority == 4


def test_workflow_with_complex_dependencies(temp_storage):
    """Test workflow with complex dependency chains."""
    workflow = Workflow(
        id="wv-deps",
        name="dependencies",
        steps=[
            WorkflowStep(title="Task A"),
            WorkflowStep(title="Task B", depends_on=["Task A"]),
            WorkflowStep(title="Task C", depends_on=["Task A", "Task B"]),
        ],
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-deps")

    assert loaded is not None
    assert loaded.steps[0].depends_on == []
    assert loaded.steps[1].depends_on == ["Task A"]
    assert loaded.steps[2].depends_on == ["Task A", "Task B"]


def test_workflow_with_multiple_labels(temp_storage):
    """Test workflow step with multiple labels."""
    workflow = Workflow(
        id="wv-labels",
        name="labels",
        steps=[
            WorkflowStep(
                title="Complex task",
                labels=["frontend", "backend", "database"],
            ),
        ],
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-labels")

    assert loaded is not None
    assert loaded.steps[0].labels == ["frontend", "backend", "database"]


def test_update_existing_workflow(temp_storage, sample_workflow):
    """Test updating an existing workflow."""
    temp_storage.write_workflow(sample_workflow)

    sample_workflow.description = "Updated description"
    sample_workflow.updated_at = datetime(2026, 1, 7, 11, 0, 0)
    temp_storage.write_workflow(sample_workflow)

    loaded = temp_storage.read_workflow(sample_workflow.id)

    assert loaded is not None
    assert loaded.description == "Updated description"
    assert loaded.updated_at == datetime(2026, 1, 7, 11, 0, 0)


def test_workflow_with_empty_description(temp_storage):
    """Test workflow with empty description."""
    workflow = Workflow(
        id="wv-nodesc",
        name="nodesc",
        description="",
    )

    temp_storage.write_workflow(workflow)
    loaded = temp_storage.read_workflow("wv-nodesc")

    assert loaded is not None
    assert loaded.description == ""


def test_yaml_format_has_correct_structure(temp_storage, sample_workflow):
    """Test that generated YAML has the correct structure."""
    temp_storage.write_workflow(sample_workflow)

    path = temp_storage.workflow_path(sample_workflow.id)
    content = path.read_text()

    # Check for correct YAML structure
    assert content.startswith("id:")
    assert "\nname:" in content
    assert "\ndescription:" in content
    assert "\ncreated_at:" in content
    assert "\nupdated_at:" in content
    assert "\nsteps:" in content
    assert "- title:" in content

    # Ensure it's pure YAML, not markdown
    assert "---" not in content
    assert "##" not in content
