"""Tests for LaunchService."""

from pathlib import Path

import pytest

from weaver.models import AgentModel, Status
from weaver.service import HintService, IssueNotFoundError, IssueService, LaunchService
from weaver.storage import HintStorage, LaunchStorage, MarkdownStorage


@pytest.fixture
def weaver_root(tmp_path: Path) -> Path:
    root = tmp_path / ".weaver"
    root.mkdir()
    return root


@pytest.fixture
def storage(weaver_root: Path) -> MarkdownStorage:
    storage = MarkdownStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def hint_storage(weaver_root: Path) -> HintStorage:
    storage = HintStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def launch_storage(weaver_root: Path) -> LaunchStorage:
    storage = LaunchStorage(weaver_root)
    storage.ensure_initialized()
    return storage


@pytest.fixture
def issue_service(storage: MarkdownStorage) -> IssueService:
    return IssueService(storage)


@pytest.fixture
def hint_service(hint_storage: HintStorage) -> HintService:
    return HintService(hint_storage)


@pytest.fixture
def launch_service(
    issue_service: IssueService,
    launch_storage: LaunchStorage,
    hint_service: HintService,
) -> LaunchService:
    return LaunchService(issue_service, launch_storage, hint_service)


class TestPrepareContext:
    def test_includes_basic_issue_details(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(
            title="Test Task",
            priority=1,
        )

        context = launch_service.prepare_context(issue)

        assert "# Task: Test Task" in context
        assert f"**ID**: {issue.id}" in context
        assert "**Priority**: P1" in context

    def test_includes_description(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(
            title="Test", description="This is a test issue description"
        )

        context = launch_service.prepare_context(issue)

        assert "## Description" in context
        assert "This is a test issue description" in context

    def test_includes_design_notes(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(title="Test")
        issue.design_notes = "Some design notes here"
        issue_service.update_issue(issue)

        context = launch_service.prepare_context(issue)

        assert "## Design Notes" in context
        assert "Some design notes here" in context

    def test_includes_acceptance_criteria(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(title="Test")
        issue.acceptance_criteria = ["Criterion 1", "Criterion 2"]
        issue_service.update_issue(issue)

        context = launch_service.prepare_context(issue)

        assert "## Acceptance Criteria" in context
        assert "- [ ] Criterion 1" in context
        assert "- [ ] Criterion 2" in context

    def test_includes_hints_from_labels(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        hint_service: HintService,
    ):
        hint_service.create_or_update_hint(
            title="python", content="Use Python best practices", labels=["python"]
        )
        hint_service.create_or_update_hint(
            title="testing", content="Write comprehensive tests", labels=["testing"]
        )

        issue = issue_service.create_issue(
            title="Test", labels=["python", "testing"]
        )

        context = launch_service.prepare_context(issue)

        assert "## Relevant Hints" in context
        assert "### python" in context
        assert "Use Python best practices" in context
        assert "### testing" in context
        assert "Write comprehensive tests" in context

    def test_no_hints_section_when_no_labels(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(title="Test", labels=[])

        context = launch_service.prepare_context(issue)

        assert "## Relevant Hints" not in context

    def test_skips_missing_hints(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        hint_service: HintService,
    ):
        hint_service.create_or_update_hint(
            title="python", content="Python hint", labels=["python"]
        )

        issue = issue_service.create_issue(
            title="Test", labels=["python", "nonexistent"]
        )

        context = launch_service.prepare_context(issue)

        assert "### python" in context
        assert "Python hint" in context
        # Should not fail even though "nonexistent" hint doesn't exist

    def test_includes_blockers_with_status(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        blocker1 = issue_service.create_issue("Blocker 1")
        blocker2 = issue_service.create_issue("Blocker 2")
        issue_service.start_issue(blocker2.id)

        blocked = issue_service.create_issue(
            "Blocked Task", blocked_by=[blocker1.id, blocker2.id]
        )

        context = launch_service.prepare_context(blocked)

        assert "## Dependencies (Blockers)" in context
        assert f"- {blocker1.id}: Blocker 1 (open)" in context
        assert f"- {blocker2.id}: Blocker 2 (in_progress)" in context

    def test_no_blockers_section_when_no_dependencies(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue("Independent Task")

        context = launch_service.prepare_context(issue)

        assert "## Dependencies (Blockers)" not in context

    def test_context_without_optional_fields(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(title="Minimal Task")

        context = launch_service.prepare_context(issue)

        # Should have basic info
        assert "# Task: Minimal Task" in context
        assert f"**ID**: {issue.id}" in context

        # Should not have optional sections
        assert "## Description" not in context
        assert "## Design Notes" not in context
        assert "## Acceptance Criteria" not in context
        assert "## Relevant Hints" not in context
        assert "## Dependencies (Blockers)" not in context

    def test_includes_workflow_instructions(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue(title="Test Task")

        context = launch_service.prepare_context(issue)

        assert "## Workflow Instructions" in context
        assert "When you have completed this task:" in context
        assert "Verify all acceptance criteria are met" in context
        assert "Run any relevant tests" in context
        assert f"weaver close {issue.id}" in context
        assert "This marks the issue as complete and unblocks any dependent tasks" in context


class TestLaunchAgent:
    def test_creates_launch_record(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        launch_storage: LaunchStorage,
    ):
        issue = issue_service.create_issue("Test Task")

        launch = launch_service.launch_agent(issue.id, AgentModel.SONNET)

        assert launch.id.startswith("wv-launch-")
        assert launch.issue_id == issue.id
        assert launch.model == AgentModel.SONNET
        assert launch.started_at is not None
        assert launch.completed_at is None
        assert launch.exit_code is None
        assert launch.log_file != ""

        # Verify it was persisted
        loaded = launch_storage.read_launch(launch.id)
        assert loaded is not None
        assert loaded.id == launch.id

    def test_creates_context_file(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        launch_storage: LaunchStorage,
    ):
        issue = issue_service.create_issue(
            title="Test Task", description="Task description"
        )

        launch = launch_service.launch_agent(issue.id, AgentModel.OPUS)

        context_file = launch_storage.logs_dir / f"{launch.id}-context.md"
        assert context_file.exists()

        context = context_file.read_text()
        assert "# Task: Test Task" in context
        assert "Task description" in context

    def test_sets_log_file_path(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        launch_storage: LaunchStorage,
    ):
        issue = issue_service.create_issue("Test Task")

        launch = launch_service.launch_agent(issue.id, AgentModel.FLASH)

        expected_log = str(launch_storage.logs_dir / f"{launch.id}.log")
        assert launch.log_file == expected_log

    def test_raises_for_missing_issue(
        self, launch_service: LaunchService
    ):
        with pytest.raises(IssueNotFoundError, match="wv-nonexistent"):
            launch_service.launch_agent("wv-nonexistent", AgentModel.SONNET)

    def test_context_includes_hints_and_blockers(
        self,
        launch_service: LaunchService,
        issue_service: IssueService,
        hint_service: HintService,
        launch_storage: LaunchStorage,
    ):
        hint_service.create_or_update_hint(
            title="python", content="Python best practices"
        )

        blocker = issue_service.create_issue("Blocker Task")
        issue = issue_service.create_issue(
            title="Main Task",
            description="Main description",
            labels=["python"],
            blocked_by=[blocker.id],
        )

        launch = launch_service.launch_agent(issue.id, AgentModel.SONNET)

        context_file = launch_storage.logs_dir / f"{launch.id}-context.md"
        context = context_file.read_text()

        # Should include hints
        assert "## Relevant Hints" in context
        assert "### python" in context
        assert "Python best practices" in context

        # Should include dependencies
        assert "## Dependencies (Blockers)" in context
        assert f"- {blocker.id}: Blocker Task" in context

    def test_works_with_different_models(
        self, launch_service: LaunchService, issue_service: IssueService
    ):
        issue = issue_service.create_issue("Test Task")

        for model in [AgentModel.SONNET, AgentModel.OPUS, AgentModel.FLASH]:
            launch = launch_service.launch_agent(issue.id, model)
            assert launch.model == model
