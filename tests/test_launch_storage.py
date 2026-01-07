"""Tests for LaunchStorage class."""

from datetime import datetime
from pathlib import Path

import pytest

from weaver.models import AgentModel, LaunchExecution
from weaver.storage import LaunchStorage


@pytest.fixture
def temp_storage_dir(tmp_path):
    """Provide a temporary storage directory."""
    return tmp_path / ".weaver"


@pytest.fixture
def launch_storage(temp_storage_dir):
    """Provide an initialized LaunchStorage instance."""
    storage = LaunchStorage(temp_storage_dir)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def sample_launch():
    """Provide a sample launch execution."""
    return LaunchExecution(
        id="wv-launch-a3f8",
        issue_id="wv-1234",
        model=AgentModel.SONNET,
        started_at=datetime(2026, 1, 7, 10, 0, 0),
        completed_at=datetime(2026, 1, 7, 10, 5, 0),
        exit_code=0,
        log_file="/path/to/.weaver/launches/logs/wv-launch-a3f8.log",
    )


def test_ensure_initialized_creates_directories(temp_storage_dir):
    """Test that ensure_initialized creates launches and logs directories."""
    storage = LaunchStorage(temp_storage_dir)
    storage.ensure_initialized()

    assert storage.launches_dir.exists()
    assert storage.logs_dir.exists()
    assert storage.launches_dir == temp_storage_dir / "launches"
    assert storage.logs_dir == temp_storage_dir / "launches" / "logs"


def test_launch_path(launch_storage):
    """Test that launch_path returns correct path."""
    path = launch_storage.launch_path("wv-launch-a3f8")
    assert path == launch_storage.launches_dir / "wv-launch-a3f8.yml"


def test_write_and_read_launch(launch_storage, sample_launch):
    """Test writing and reading a launch execution."""
    launch_storage.write_launch(sample_launch)

    path = launch_storage.launch_path(sample_launch.id)
    assert path.exists()

    read_launch = launch_storage.read_launch(sample_launch.id)
    assert read_launch is not None
    assert read_launch.id == sample_launch.id
    assert read_launch.issue_id == sample_launch.issue_id
    assert read_launch.model == sample_launch.model
    assert read_launch.started_at == sample_launch.started_at
    assert read_launch.completed_at == sample_launch.completed_at
    assert read_launch.exit_code == sample_launch.exit_code
    assert read_launch.log_file == sample_launch.log_file


def test_read_nonexistent_launch(launch_storage):
    """Test reading a launch that doesn't exist returns None."""
    result = launch_storage.read_launch("nonexistent")
    assert result is None


def test_write_launch_with_none_values(launch_storage):
    """Test writing a launch with None values for optional fields."""
    launch = LaunchExecution(
        id="wv-launch-incomplete",
        issue_id="wv-5678",
        model=AgentModel.OPUS,
        started_at=datetime(2026, 1, 7, 11, 0, 0),
        completed_at=None,
        exit_code=None,
        log_file="",
    )

    launch_storage.write_launch(launch)
    read_launch = launch_storage.read_launch(launch.id)

    assert read_launch is not None
    assert read_launch.id == launch.id
    assert read_launch.issue_id == launch.issue_id
    assert read_launch.model == launch.model
    assert read_launch.started_at == launch.started_at
    assert read_launch.completed_at is None
    assert read_launch.exit_code is None
    assert read_launch.log_file == ""


def test_write_launch_with_different_models(launch_storage):
    """Test writing launches with different AgentModel enum values."""
    models = [AgentModel.SONNET, AgentModel.OPUS, AgentModel.FLASH]

    for i, model in enumerate(models):
        launch = LaunchExecution(
            id=f"wv-launch-{i}",
            issue_id="wv-model-test",
            model=model,
            started_at=datetime(2026, 1, 7, 12, i, 0),
        )
        launch_storage.write_launch(launch)

        read_launch = launch_storage.read_launch(launch.id)
        assert read_launch is not None
        assert read_launch.model == model


def test_list_launches_for_issue(launch_storage):
    """Test listing all launches for a specific issue."""
    issue_id = "wv-1234"
    other_issue_id = "wv-5678"

    launches_for_issue = [
        LaunchExecution(
            id="wv-launch-1",
            issue_id=issue_id,
            model=AgentModel.SONNET,
            started_at=datetime(2026, 1, 7, 10, 0, 0),
        ),
        LaunchExecution(
            id="wv-launch-2",
            issue_id=issue_id,
            model=AgentModel.OPUS,
            started_at=datetime(2026, 1, 7, 11, 0, 0),
        ),
        LaunchExecution(
            id="wv-launch-3",
            issue_id=issue_id,
            model=AgentModel.FLASH,
            started_at=datetime(2026, 1, 7, 12, 0, 0),
        ),
    ]

    other_launch = LaunchExecution(
        id="wv-launch-other",
        issue_id=other_issue_id,
        model=AgentModel.SONNET,
        started_at=datetime(2026, 1, 7, 13, 0, 0),
    )

    for launch in launches_for_issue:
        launch_storage.write_launch(launch)
    launch_storage.write_launch(other_launch)

    result = launch_storage.list_launches_for_issue(issue_id)

    assert len(result) == 3
    assert all(launch.issue_id == issue_id for launch in result)
    result_ids = {launch.id for launch in result}
    assert result_ids == {"wv-launch-1", "wv-launch-2", "wv-launch-3"}


def test_list_launches_for_nonexistent_issue(launch_storage):
    """Test listing launches for an issue with no launches."""
    result = launch_storage.list_launches_for_issue("wv-nonexistent")
    assert result == []


def test_list_launches_when_directory_does_not_exist(temp_storage_dir):
    """Test listing launches when launches directory doesn't exist."""
    storage = LaunchStorage(temp_storage_dir)
    result = storage.list_launches_for_issue("wv-1234")
    assert result == []


def test_yaml_format(launch_storage, sample_launch):
    """Test that YAML file has expected structure."""
    launch_storage.write_launch(sample_launch)

    path = launch_storage.launch_path(sample_launch.id)
    content = path.read_text()

    assert "id: wv-launch-a3f8" in content
    assert "issue_id: wv-1234" in content
    assert "model: claude-sonnet-4-5-20250929" in content
    assert "started_at: '2026-01-07T10:00:00'" in content
    assert "completed_at: '2026-01-07T10:05:00'" in content
    assert "exit_code: 0" in content
    assert "log_file: /path/to/.weaver/launches/logs/wv-launch-a3f8.log" in content


def test_yaml_format_with_none_completed_at(launch_storage):
    """Test YAML format when completed_at is None."""
    launch = LaunchExecution(
        id="wv-launch-running",
        issue_id="wv-9999",
        model=AgentModel.SONNET,
        started_at=datetime(2026, 1, 7, 15, 0, 0),
        completed_at=None,
    )

    launch_storage.write_launch(launch)

    path = launch_storage.launch_path(launch.id)
    content = path.read_text()

    assert "completed_at: null" in content or "completed_at:" not in content


def test_overwrite_existing_launch(launch_storage, sample_launch):
    """Test that writing a launch with same ID overwrites the previous one."""
    launch_storage.write_launch(sample_launch)

    modified_launch = LaunchExecution(
        id=sample_launch.id,
        issue_id=sample_launch.issue_id,
        model=AgentModel.OPUS,
        started_at=sample_launch.started_at,
        completed_at=datetime(2026, 1, 7, 10, 10, 0),
        exit_code=1,
        log_file="/different/path.log",
    )

    launch_storage.write_launch(modified_launch)

    read_launch = launch_storage.read_launch(sample_launch.id)
    assert read_launch is not None
    assert read_launch.model == AgentModel.OPUS
    assert read_launch.exit_code == 1
    assert read_launch.log_file == "/different/path.log"
