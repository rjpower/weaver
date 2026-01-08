"""Tests for weaver.cli."""

import os
from pathlib import Path
from unittest.mock import Mock, mock_open, patch

import pytest
from click.testing import CliRunner

from weaver.cli import cli
from weaver.models import Status
from weaver.service import HintService, IssueService
from weaver.storage import HintStorage, MarkdownStorage


@pytest.fixture
def runner():
    return CliRunner()


@pytest.fixture
def weaver_dir(tmp_path: Path):
    """Create a weaver project directory and change to it."""
    weaver_root = tmp_path / ".weaver"
    weaver_root.mkdir()
    (weaver_root / "issues").mkdir()
    storage = MarkdownStorage(weaver_root)
    storage.ensure_initialized()
    return tmp_path


@pytest.fixture
def service(weaver_dir: Path) -> IssueService:
    """Create service for the test weaver directory."""
    return IssueService(MarkdownStorage(weaver_dir / ".weaver"))


@pytest.fixture
def hint_service(weaver_dir: Path) -> HintService:
    """Create hint service for the test weaver directory."""
    hint_storage = HintStorage(weaver_dir / ".weaver")
    hint_storage.ensure_initialized()
    return HintService(hint_storage)


class TestInit:
    def test_creates_weaver_directory(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init"])
            assert result.exit_code == 0
            assert "Initialized" in result.output
            assert (Path.cwd() / ".weaver" / "issues").is_dir()

    def test_idempotent(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            runner.invoke(cli, ["init"])
            result = runner.invoke(cli, ["init"])
            assert result.exit_code == 0
            assert "already initialized" in result.output


class TestCreate:
    def test_creates_issue(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["create", "Test issue"])

        assert result.exit_code == 0
        assert "Created" in result.output
        assert "wv-" in result.output

        issues = service.list_issues()
        assert len(issues) == 1
        assert issues[0].title == "Test issue"

    def test_with_blocked_by(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        blocker = service.create_issue("Blocker")

        result = runner.invoke(cli, ["create", "Blocked", "-b", blocker.id])

        assert result.exit_code == 0
        issues = service.list_issues()
        blocked = next(i for i in issues if i.title == "Blocked")
        assert blocker.id in blocked.blocked_by

    def test_rejects_invalid_blocked_by(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["create", "Test", "-b", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "non-existent" in result.output

    def test_with_file_description(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        desc_file = weaver_dir / "description.md"
        desc_file.write_text("This is a detailed description\nwith multiple lines.")

        result = runner.invoke(cli, ["create", "Issue from file", "-f", str(desc_file)])

        assert result.exit_code == 0
        issues = service.list_issues()
        assert issues[0].description == "This is a detailed description\nwith multiple lines."

    def test_with_stdin_description(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)

        result = runner.invoke(
            cli,
            ["create", "Issue from stdin", "-f", "-"],
            input="Description from stdin\nLine 2",
        )

        assert result.exit_code == 0
        issues = service.list_issues()
        assert issues[0].description == "Description from stdin\nLine 2"

    def test_file_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["create", "Test", "-f", "/nonexistent/path.md"])

        assert result.exit_code != 0
        assert "File not found" in result.output

    def test_file_overrides_description_flag(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        desc_file = weaver_dir / "description.md"
        desc_file.write_text("From file")

        result = runner.invoke(
            cli,
            ["create", "Test", "-d", "From flag", "-f", str(desc_file)],
        )

        assert result.exit_code == 0
        issues = service.list_issues()
        assert issues[0].description == "From file"


class TestShow:
    def test_shows_issue(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue(
            "Test issue",
            priority=1,
            labels=["backend"],
        )

        result = runner.invoke(cli, ["show", issue.id])

        assert result.exit_code == 0
        assert issue.id in result.output
        assert "Test issue" in result.output
        assert "P1" in result.output
        assert "backend" in result.output

    def test_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["show", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "not found" in result.output

    def test_fetch_deps_displays_dependencies_in_topological_order(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService
    ):
        os.chdir(weaver_dir)
        # Create dependency chain: main -> dep1 -> dep2
        dep2 = service.create_issue("Deepest dependency", description="This is the deepest dependency")
        dep1 = service.create_issue("Middle dependency", description="Depends on dep2", blocked_by=[dep2.id])
        main = service.create_issue("Main issue", description="Main issue content", blocked_by=[dep1.id])

        result = runner.invoke(cli, ["show", main.id, "--fetch-deps"])

        assert result.exit_code == 0
        # Dependencies section should appear
        assert "Dependencies (topological order - deepest first):" in result.output
        # All three issues should appear
        assert dep2.id in result.output
        assert dep1.id in result.output
        assert main.id in result.output
        # Dependencies should appear before main issue
        assert "Main Issue:" in result.output
        dep_section_pos = result.output.index("Dependencies")
        main_section_pos = result.output.index("Main Issue:")
        assert dep_section_pos < main_section_pos
        # Deepest dependency should appear first
        dep2_pos = result.output.index(dep2.id)
        dep1_pos = result.output.index(dep1.id)
        assert dep2_pos < dep1_pos

    def test_fetch_deps_truncates_long_content(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        # Create a dependency with long description (more than 200 words)
        long_description = " ".join([f"word{i}" for i in range(250)])
        dep = service.create_issue("Dependency", description=long_description)
        main = service.create_issue("Main", blocked_by=[dep.id])

        result = runner.invoke(cli, ["show", main.id, "--fetch-deps"])

        assert result.exit_code == 0
        # Should show truncation indicator
        assert "..." in result.output
        # Should show hint about using show command
        assert f"Use 'weaver show {dep.id}' to see complete content" in result.output
        # Should not contain all 250 words
        assert "word249" not in result.output

    def test_fetch_deps_shows_full_content_if_short(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        # Create a dependency with short description
        short_description = "This is a short description with few words"
        dep = service.create_issue("Dependency", description=short_description)
        main = service.create_issue("Main", blocked_by=[dep.id])

        result = runner.invoke(cli, ["show", main.id, "--fetch-deps"])

        assert result.exit_code == 0
        # Should show full content
        assert short_description in result.output
        # Should not show truncation hint
        assert "to see complete content" not in result.output

    def test_fetch_deps_combines_description_and_design_notes(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService
    ):
        os.chdir(weaver_dir)
        # Create dependency with both description and design notes
        dep = service.create_issue("Dependency", description="Description part")
        dep.design_notes = "Design notes part"
        service.update_issue(dep)
        main = service.create_issue("Main", blocked_by=[dep.id])

        result = runner.invoke(cli, ["show", main.id, "--fetch-deps"])

        assert result.exit_code == 0
        # Both parts should appear in dependencies section
        assert "Description part" in result.output
        assert "Design notes part" in result.output

    def test_fetch_deps_handles_no_dependencies(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        # Create an issue with no dependencies
        issue = service.create_issue("Standalone", description="No dependencies")

        result = runner.invoke(cli, ["show", issue.id, "--fetch-deps"])

        assert result.exit_code == 0
        # Should not show dependencies section
        assert "Dependencies (topological order - deepest first):" not in result.output
        # Should still show main issue
        assert "Main Issue:" in result.output
        assert "Standalone" in result.output

    def test_fetch_deps_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["show", "wv-nonexistent", "--fetch-deps"])

        assert result.exit_code != 0
        assert "not found" in result.output

    def test_without_fetch_deps_works_as_before(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        # Create dependency chain
        dep = service.create_issue("Dependency")
        main = service.create_issue("Main", blocked_by=[dep.id])

        result = runner.invoke(cli, ["show", main.id])

        assert result.exit_code == 0
        # Should not show dependencies section
        assert "Dependencies (topological order" not in result.output
        assert "Main Issue:" not in result.output
        # Should show main issue info
        assert main.id in result.output
        assert "Main" in result.output


class TestList:
    def test_lists_all_open_by_default(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        service.create_issue("Issue 1")
        service.create_issue("Issue 2")
        closed_issue = service.create_issue("Closed Issue")
        service.close_issue(closed_issue.id)

        result = runner.invoke(cli, ["list"])

        assert result.exit_code == 0
        assert "Issue 1" in result.output
        assert "Issue 2" in result.output
        assert "Closed Issue" not in result.output

    def test_lists_all_with_flag(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        service.create_issue("Issue 1")
        service.create_issue("Issue 2")
        closed_issue = service.create_issue("Closed Issue")
        service.close_issue(closed_issue.id)

        result = runner.invoke(cli, ["list", "--all"])

        assert result.exit_code == 0
        assert "Issue 1" in result.output
        assert "Issue 2" in result.output
        assert "Closed Issue" in result.output

    def test_filters_by_status(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        open_issue = service.create_issue("Open")
        closed_issue = service.create_issue("Closed")
        service.close_issue(closed_issue.id)

        result = runner.invoke(cli, ["list", "-s", "open"])

        assert result.exit_code == 0
        assert "Open" in result.output
        assert "Closed" not in result.output

    def test_filters_by_status_closed(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        open_issue = service.create_issue("Open")
        closed_issue = service.create_issue("Closed")
        service.close_issue(closed_issue.id)

        result = runner.invoke(cli, ["list", "-s", "closed"])

        assert result.exit_code == 0
        assert "Closed" in result.output
        assert "Open" not in result.output

    def test_filters_by_label(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        service.create_issue("Backend", labels=["backend"])
        service.create_issue("Frontend", labels=["frontend"])

        result = runner.invoke(cli, ["list", "-l", "backend"])

        assert result.exit_code == 0
        assert "Backend" in result.output
        assert "Frontend" not in result.output

    def test_empty_list(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["list"])

        assert result.exit_code == 0
        assert "No issues found" in result.output


class TestReady:
    def test_shows_unblocked(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        blocker = service.create_issue("Blocker")
        service.create_issue("Blocked", blocked_by=[blocker.id])

        result = runner.invoke(cli, ["ready"])

        assert result.exit_code == 0
        assert "Blocker" in result.output
        assert "Blocked" not in result.output

    def test_filters_by_label(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        service.create_issue("Backend", labels=["backend"])
        service.create_issue("Frontend", labels=["frontend"])

        result = runner.invoke(cli, ["ready", "-l", "backend"])

        assert result.exit_code == 0
        assert "Backend" in result.output
        assert "Frontend" not in result.output

    def test_respects_limit(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        for i in range(5):
            service.create_issue(f"Issue {i}")

        result = runner.invoke(cli, ["ready", "-n", "2"])

        assert result.exit_code == 0
        # Count the number of issue rows (not header)
        lines = [l for l in result.output.split("\n") if "Issue" in l]
        assert len(lines) == 2


class TestStart:
    def test_starts_issue(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("To start")

        result = runner.invoke(cli, ["start", issue.id])

        assert result.exit_code == 0
        assert "Started" in result.output

        updated = service.get_issue(issue.id)
        assert updated is not None
        assert updated.status == Status.IN_PROGRESS

    def test_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["start", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "not found" in result.output


class TestClose:
    def test_closes_issue(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("To close")

        result = runner.invoke(cli, ["close", issue.id])

        assert result.exit_code == 0
        assert "Closed" in result.output

        updated = service.get_issue(issue.id)
        assert updated is not None
        assert updated.status == Status.CLOSED

    def test_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["close", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "not found" in result.output


class TestDepAdd:
    def test_adds_dependency(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked")

        result = runner.invoke(cli, ["dep", "add", blocked.id, blocker.id])

        assert result.exit_code == 0
        assert "blocked by" in result.output

        updated = service.get_issue(blocked.id)
        assert updated is not None
        assert blocker.id in updated.blocked_by

    def test_rejects_cycle(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        a = service.create_issue("A")
        b = service.create_issue("B")
        service.add_dependency(b.id, a.id)

        result = runner.invoke(cli, ["dep", "add", a.id, b.id])

        assert result.exit_code != 0
        assert "cycle" in result.output

    def test_not_found(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Issue")

        result = runner.invoke(cli, ["dep", "add", issue.id, "wv-nonexistent"])

        assert result.exit_code != 0
        assert "not found" in result.output


class TestDepRm:
    def test_removes_dependency(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        blocker = service.create_issue("Blocker")
        blocked = service.create_issue("Blocked", blocked_by=[blocker.id])

        result = runner.invoke(cli, ["dep", "rm", blocked.id, blocker.id])

        assert result.exit_code == 0
        assert "no longer blocked" in result.output

        updated = service.get_issue(blocked.id)
        assert updated is not None
        assert blocker.id not in updated.blocked_by

    def test_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["dep", "rm", "wv-nonexistent", "wv-other"])

        assert result.exit_code != 0
        assert "not found" in result.output


class TestNotInitialized:
    def test_fails_without_init(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["list"])
            assert result.exit_code != 0
            assert "Run 'weaver init' first" in result.output


class TestReadme:
    def test_shows_help_text(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["readme"])

        assert result.exit_code == 0
        assert "Weaver" in result.output
        assert "Quick Reference" in result.output
        assert "weaver init" in result.output
        assert "weaver create" in result.output
        assert "weaver sync" in result.output


class TestInitGitignore:
    def test_creates_gitignore(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init"])

            assert result.exit_code == 0
            gitignore_path = Path.cwd() / ".gitignore"
            assert gitignore_path.exists()
            content = gitignore_path.read_text()
            assert ".weaver/" in content

    def test_appends_to_existing_gitignore(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            gitignore_path = Path.cwd() / ".gitignore"
            gitignore_path.write_text("*.pyc\n__pycache__/\n")

            result = runner.invoke(cli, ["init"])

            assert result.exit_code == 0
            content = gitignore_path.read_text()
            assert "*.pyc" in content
            assert ".weaver/" in content

    def test_does_not_duplicate_entry(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            gitignore_path = Path.cwd() / ".gitignore"
            gitignore_path.write_text(".weaver/\n")

            result = runner.invoke(cli, ["init"])

            assert result.exit_code == 0
            content = gitignore_path.read_text()
            assert content.count(".weaver/") == 1


class TestSync:
    @pytest.fixture
    def git_weaver_dir(self, weaver_dir: Path):
        """Create a weaver project inside a git repository."""
        import subprocess

        os.chdir(weaver_dir)
        subprocess.run(["git", "init"], capture_output=True)
        subprocess.run(["git", "config", "user.email", "test@test.com"], capture_output=True)
        subprocess.run(["git", "config", "user.name", "Test"], capture_output=True)
        # Create initial gitignore with .weaver/
        gitignore_path = weaver_dir / ".gitignore"
        gitignore_path.write_text(".weaver/\n")
        subprocess.run(["git", "add", "."], capture_output=True)
        subprocess.run(["git", "commit", "-m", "init"], capture_output=True)
        return weaver_dir

    def test_shows_sync_branch(self, runner: CliRunner, git_weaver_dir: Path):
        result = runner.invoke(cli, ["sync"])

        assert result.exit_code == 0
        assert "weaver-" in result.output
        assert "Sync branch" in result.output

    def test_flips_gitignore(self, runner: CliRunner, git_weaver_dir: Path):
        result = runner.invoke(cli, ["sync"])

        assert result.exit_code == 0
        gitignore_path = git_weaver_dir / ".gitignore"
        content = gitignore_path.read_text()
        assert ".weaver/index.yml" in content
        assert ".weaver/\n" not in content

    def test_custom_branch(self, runner: CliRunner, git_weaver_dir: Path):
        result = runner.invoke(cli, ["sync", "-b", "my-issues"])

        assert result.exit_code == 0
        assert "my-issues" in result.output

    def test_requires_git_repo(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["sync"])

        assert result.exit_code != 0
        assert "git repository" in result.output

    def test_shows_issue_count(self, runner: CliRunner, git_weaver_dir: Path, service: IssueService):
        service.create_issue("Issue 1")
        service.create_issue("Issue 2")

        result = runner.invoke(cli, ["sync"])

        assert result.exit_code == 0
        assert "Local issues: 2" in result.output


class TestHintAdd:
    def test_add_hint_from_stdin(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        result = runner.invoke(
            cli,
            ["hint", "add", "test-hint", "-f", "-"],
            input="This is test content\nwith multiple lines",
        )

        assert result.exit_code == 0
        assert "Created hint" in result.output
        assert "test-hint" in result.output
        assert "wv-hint-" in result.output

        hints = hint_service.list_hints()
        assert len(hints) == 1
        assert hints[0].title == "test-hint"
        assert hints[0].content == "This is test content\nwith multiple lines"

    def test_add_hint_from_file(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        content_file = weaver_dir / "hint_content.txt"
        content_file.write_text("File content here")

        result = runner.invoke(cli, ["hint", "add", "file-hint", "-f", str(content_file)])

        assert result.exit_code == 0
        assert "Created hint" in result.output

        hints = hint_service.list_hints()
        assert len(hints) == 1
        assert hints[0].content == "File content here"

    def test_add_hint_with_labels(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        result = runner.invoke(
            cli,
            ["hint", "add", "labeled-hint", "-l", "auth", "-l", "backend", "-f", "-"],
            input="Content with labels",
        )

        assert result.exit_code == 0

        hints = hint_service.list_hints()
        assert len(hints) == 1
        assert set(hints[0].labels) == {"auth", "backend"}

    def test_update_existing_hint(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)

        # Create initial hint
        result1 = runner.invoke(
            cli,
            ["hint", "add", "update-test", "-f", "-"],
            input="Original content",
        )
        assert result1.exit_code == 0
        assert "Created hint" in result1.output

        # Update the same hint
        result2 = runner.invoke(
            cli,
            ["hint", "add", "update-test", "-f", "-"],
            input="Updated content",
        )
        assert result2.exit_code == 0
        assert "Updated hint" in result2.output

        hints = hint_service.list_hints()
        assert len(hints) == 1
        assert hints[0].content == "Updated content"

    def test_requires_content_flag(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["hint", "add", "no-content"])

        assert result.exit_code != 0
        assert "Content required" in result.output

    def test_file_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["hint", "add", "test", "-f", "/nonexistent/file.txt"])

        assert result.exit_code != 0
        assert "File not found" in result.output

    def test_not_in_weaver_project(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["hint", "add", "test", "-f", "-"], input="content")
            assert result.exit_code != 0
            assert "Not in a weaver project" in result.output


class TestHintShow:
    def test_show_hint_by_title(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint = hint_service.create_or_update_hint("show-test", "Content to display", ["label1"])

        result = runner.invoke(cli, ["hint", "show", "show-test"])

        assert result.exit_code == 0
        assert "show-test" in result.output
        assert hint.id in result.output
        assert "Content to display" in result.output
        assert "label1" in result.output

    def test_show_hint_by_id(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint = hint_service.create_or_update_hint("id-test", "Content by ID")

        result = runner.invoke(cli, ["hint", "show", hint.id])

        assert result.exit_code == 0
        assert "id-test" in result.output
        assert "Content by ID" in result.output

    def test_show_hint_not_found(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["hint", "show", "nonexistent"])

        assert result.exit_code != 0
        assert "Hint not found" in result.output

    def test_show_hint_without_labels(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint_service.create_or_update_hint("no-labels", "Content without labels", [])

        result = runner.invoke(cli, ["hint", "show", "no-labels"])

        assert result.exit_code == 0
        assert "Content without labels" in result.output


class TestHintList:
    def test_list_all_hints(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint_service.create_or_update_hint("hint1", "Content 1", ["label1"])
        hint_service.create_or_update_hint("hint2", "Content 2", ["label2"])

        result = runner.invoke(cli, ["hint", "list"])

        assert result.exit_code == 0
        assert "hint1" in result.output
        assert "hint2" in result.output
        assert "label1" in result.output
        assert "label2" in result.output

    def test_list_empty(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["hint", "list"])

        assert result.exit_code == 0
        assert "No hints found" in result.output

    def test_list_shows_ids(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint = hint_service.create_or_update_hint("test", "Content")

        result = runner.invoke(cli, ["hint", "list"])

        assert result.exit_code == 0
        assert hint.id in result.output


class TestHintSearch:
    def test_search_finds_matching_hints(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint_service.create_or_update_hint("auth-hint", "Authentication setup guide")
        hint_service.create_or_update_hint("db-hint", "Database configuration")

        result = runner.invoke(cli, ["hint", "search", "auth"])

        assert result.exit_code == 0
        assert "auth-hint" in result.output
        assert "db-hint" not in result.output

    def test_search_truncates_long_content(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        long_content = " ".join([f"word{i}" for i in range(100)])
        hint_service.create_or_update_hint("long-hint", long_content)

        result = runner.invoke(cli, ["hint", "search", "word"])

        assert result.exit_code == 0
        assert "long-hint" in result.output
        assert "..." in result.output
        # Should not contain all 100 words
        assert "word99" not in result.output

    def test_search_no_results(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint_service.create_or_update_hint("test", "Content here")

        result = runner.invoke(cli, ["hint", "search", "nonexistent"])

        assert result.exit_code == 0
        assert "No hints found matching 'nonexistent'" in result.output

    def test_search_shows_multiple_results(self, runner: CliRunner, weaver_dir: Path, hint_service: HintService):
        os.chdir(weaver_dir)
        hint_service.create_or_update_hint("test1", "Python testing guide")
        hint_service.create_or_update_hint("test2", "Python linting setup")

        result = runner.invoke(cli, ["hint", "search", "Python"])

        assert result.exit_code == 0
        assert "test1" in result.output
        assert "test2" in result.output


class TestHintInitialization:
    def test_init_creates_hints_directory(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["init"])
            assert result.exit_code == 0
            assert (Path.cwd() / ".weaver" / "hints").is_dir()
            assert (Path.cwd() / ".weaver" / "hints_index.yml").exists()


class TestWorkflowCreate:
    def test_creates_workflow_from_stdin(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_content = """
name: test-workflow
description: A test workflow
steps:
  - title: First step
    type: task
    priority: 1
  - title: Second step
    type: bug
    priority: 2
"""
        result = runner.invoke(cli, ["workflow", "create", "test-workflow", "-f", "-"], input=yaml_content)

        assert result.exit_code == 0
        assert "Created workflow" in result.output
        assert "test-workflow" in result.output
        assert "Steps: 2" in result.output

    def test_creates_workflow_from_file(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_file = weaver_dir / "workflow.yml"
        yaml_file.write_text("""
name: file-workflow
description: From file
steps:
  - title: Task 1
    type: task
""")

        result = runner.invoke(cli, ["workflow", "create", "file-workflow", "-f", str(yaml_file)])

        assert result.exit_code == 0
        assert "Created workflow" in result.output
        assert "file-workflow" in result.output
        assert "Steps: 1" in result.output

    def test_requires_file_flag(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "create", "test"])

        assert result.exit_code != 0
        assert "Workflow YAML required" in result.output

    def test_file_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "create", "test", "-f", "/nonexistent.yml"])

        assert result.exit_code != 0
        assert "File not found" in result.output

    def test_invalid_yaml(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "create", "test", "-f", "-"], input="invalid: yaml: content:")

        assert result.exit_code != 0
        assert "Failed to parse workflow" in result.output

    def test_updates_existing_workflow(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_v1 = """
name: update-test
steps:
  - title: Step 1
"""
        yaml_v2 = """
name: update-test
steps:
  - title: Step 1
  - title: Step 2
"""
        # Create first version
        result1 = runner.invoke(cli, ["workflow", "create", "update-test", "-f", "-"], input=yaml_v1)
        assert result1.exit_code == 0

        # Update with second version
        result2 = runner.invoke(cli, ["workflow", "create", "update-test", "-f", "-"], input=yaml_v2)
        assert result2.exit_code == 0
        assert "Steps: 2" in result2.output


class TestWorkflowExecute:
    def test_executes_workflow(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        yaml_content = """
name: exec-test
steps:
  - title: First task
    type: task
    priority: 1
  - title: Second task
    type: bug
    priority: 2
"""
        # Create workflow
        runner.invoke(cli, ["workflow", "create", "exec-test", "-f", "-"], input=yaml_content)

        # Execute workflow
        result = runner.invoke(cli, ["workflow", "execute", "exec-test"])

        assert result.exit_code == 0
        assert "Created 2 issues" in result.output
        assert "exec-test" in result.output
        assert "First task" in result.output
        assert "Second task" in result.output

        # Verify issues were created
        issues = service.list_issues()
        assert len(issues) == 2
        assert any(i.title == "First task" for i in issues)
        assert any(i.title == "Second task" for i in issues)

    def test_executes_workflow_with_dependencies(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        yaml_content = """
name: deps-test
steps:
  - title: Foundation
    type: task
  - title: Build on foundation
    type: task
    depends_on:
      - Foundation
"""
        # Create and execute workflow
        runner.invoke(cli, ["workflow", "create", "deps-test", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "execute", "deps-test"])

        assert result.exit_code == 0
        assert "Created 2 issues" in result.output

        # Verify dependencies
        issues = service.list_issues()
        build_issue = next(i for i in issues if i.title == "Build on foundation")
        foundation_issue = next(i for i in issues if i.title == "Foundation")
        assert foundation_issue.id in build_issue.blocked_by

    def test_executes_workflow_with_custom_label(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        yaml_content = """
name: label-test
steps:
  - title: Task with label
    labels:
      - existing-label
"""
        runner.invoke(cli, ["workflow", "create", "label-test", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "execute", "label-test", "--label", "custom"])

        assert result.exit_code == 0

        # Verify labels
        issues = service.list_issues()
        assert len(issues) == 1
        assert "existing-label" in issues[0].labels
        assert "workflow:custom" in issues[0].labels

    def test_executes_workflow_with_default_label(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        yaml_content = """
name: default-label
steps:
  - title: Task
"""
        runner.invoke(cli, ["workflow", "create", "default-label", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "execute", "default-label"])

        assert result.exit_code == 0

        # Verify default workflow label
        issues = service.list_issues()
        assert len(issues) == 1
        assert "workflow:default-label" in issues[0].labels

    def test_workflow_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "execute", "nonexistent"])

        assert result.exit_code != 0
        assert "Workflow not found" in result.output

    def test_invalid_dependency(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_content = """
name: bad-deps
steps:
  - title: Task
    depends_on:
      - NonexistentStep
"""
        runner.invoke(cli, ["workflow", "create", "bad-deps", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "execute", "bad-deps"])

        assert result.exit_code != 0
        assert "not found in workflow" in result.output


class TestWorkflowShow:
    def test_shows_workflow_details(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_content = """
name: show-test
description: A workflow to show
steps:
  - title: First step
    type: task
    priority: 1
    labels:
      - test-label
  - title: Second step
    type: bug
    priority: 2
    depends_on:
      - First step
"""
        runner.invoke(cli, ["workflow", "create", "show-test", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "show", "show-test"])

        assert result.exit_code == 0
        assert "show-test" in result.output
        assert "A workflow to show" in result.output
        assert "Steps (2)" in result.output
        assert "First step" in result.output
        assert "Second step" in result.output
        assert "Priority: P1" in result.output
        assert "Priority: P2" in result.output
        assert "Depends on: First step" in result.output
        assert "Labels: test-label" in result.output

    def test_shows_workflow_without_description(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        yaml_content = """
name: no-desc
steps:
  - title: Step
"""
        runner.invoke(cli, ["workflow", "create", "no-desc", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "show", "no-desc"])

        assert result.exit_code == 0
        assert "no-desc" in result.output
        assert "Steps (1)" in result.output

    def test_workflow_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "show", "nonexistent"])

        assert result.exit_code != 0
        assert "Workflow not found" in result.output


class TestWorkflowList:
    def test_lists_all_workflows(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)

        # Create multiple workflows
        yaml1 = """
name: workflow-1
description: First workflow
steps:
  - title: Step
"""
        yaml2 = """
name: workflow-2
description: Second workflow with a long description that should be truncated
steps:
  - title: Step 1
  - title: Step 2
"""
        runner.invoke(cli, ["workflow", "create", "workflow-1", "-f", "-"], input=yaml1)
        runner.invoke(cli, ["workflow", "create", "workflow-2", "-f", "-"], input=yaml2)

        result = runner.invoke(cli, ["workflow", "list"])

        assert result.exit_code == 0
        assert "workflow-1" in result.output
        assert "workflow-2" in result.output
        assert "First workflow" in result.output
        assert "1" in result.output  # Step count
        assert "2" in result.output  # Step count

    def test_empty_list(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["workflow", "list"])

        assert result.exit_code == 0
        assert "No workflows found" in result.output

    def test_truncates_long_description(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        long_desc = "a" * 100
        yaml_content = f"""
name: long-desc
description: {long_desc}
steps:
  - title: Step
"""
        runner.invoke(cli, ["workflow", "create", "long-desc", "-f", "-"], input=yaml_content)
        result = runner.invoke(cli, ["workflow", "list"])

        assert result.exit_code == 0
        # Description should be truncated to 50 chars
        assert "long-desc" in result.output
        # The full 100-char description should not be in the output
        assert long_desc not in result.output
        # Should show truncation indicator
        assert "…" in result.output or "..." in result.output


@pytest.mark.skip
class TestLaunch:
    def test_launches_agent_with_default_model(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task", description="Test description")

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result) as mock_run:
            result = runner.invoke(cli, ["launch", issue.id])

        assert result.exit_code == 0
        assert "Launching sonnet agent" in result.output
        assert issue.id in result.output
        assert "Test task" in result.output
        assert "Agent completed successfully" in result.output

        # Verify subprocess was called with correct arguments
        mock_run.assert_called_once()
        call_args = mock_run.call_args[0][0]
        assert call_args[0] == "claude"
        assert "--model" in call_args
        assert "claude-sonnet-4-5-20250929" in call_args
        assert "--dangerously-skip-display" in call_args

    def test_launches_agent_with_opus_model(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result) as mock_run:
            result = runner.invoke(cli, ["launch", issue.id, "--model", "opus"])

        assert result.exit_code == 0
        assert "Launching opus agent" in result.output

        # Verify opus model was used
        call_args = mock_run.call_args[0][0]
        assert "claude-opus-4-5-20251101" in call_args

    def test_launches_agent_with_flash_model(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result) as mock_run:
            result = runner.invoke(cli, ["launch", issue.id, "--model", "flash"])

        assert result.exit_code == 0
        assert "Launching flash agent" in result.output

        # Verify flash model was used
        call_args = mock_run.call_args[0][0]
        assert "claude-3-5-haiku-20241022" in call_args

    def test_creates_context_file(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task", description="Task description", labels=["backend"])

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            runner.invoke(cli, ["launch", issue.id])

        # Verify context file was created
        context_files = list((weaver_dir / ".weaver" / "launches" / "logs").glob("*-context.md"))
        assert len(context_files) == 1
        context_content = context_files[0].read_text()
        assert "Test task" in context_content
        assert "Task description" in context_content

    def test_creates_log_file(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            result = runner.invoke(cli, ["launch", issue.id])

        assert result.exit_code == 0
        # Verify log file path was shown
        assert "Logs:" in result.output
        assert ".log" in result.output

        # Verify log file was created
        log_files = list((weaver_dir / ".weaver" / "launches" / "logs").glob("*.log"))
        assert len(log_files) == 1

    def test_handles_nonzero_exit_code(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        mock_result = Mock()
        mock_result.returncode = 1

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            result = runner.invoke(cli, ["launch", issue.id])

        assert result.exit_code == 0  # CLI itself shouldn't fail
        assert "Agent exited with code 1" in result.output
        assert "Check logs:" in result.output

    def test_handles_claude_not_found(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        with patch("weaver.cli.subprocess.run", side_effect=FileNotFoundError):
            result = runner.invoke(cli, ["launch", issue.id])

        assert result.exit_code != 0
        assert "Claude CLI not found" in result.output
        assert "pip install claude-code" in result.output

    def test_handles_invalid_issue(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["launch", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "Issue wv-nonexistent not found" in result.output

    def test_saves_launch_record(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        issue = service.create_issue("Test task")

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            runner.invoke(cli, ["launch", issue.id])

        # Verify launch record was saved
        launch_files = list((weaver_dir / ".weaver" / "launches").glob("*.yml"))
        assert len(launch_files) == 1

        # Read and verify launch record content
        import yaml

        with open(launch_files[0]) as f:
            launch_data = yaml.safe_load(f)

        assert launch_data["issue_id"] == issue.id
        assert launch_data["model"] == "claude-sonnet-4-5-20250929"
        assert launch_data["exit_code"] == 0
        assert launch_data["completed_at"] is not None

    def test_includes_hints_in_context(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService, hint_service: HintService
    ):
        os.chdir(weaver_dir)

        # Create hint with matching label
        hint_service.create_or_update_hint("backend", "Backend implementation guide", ["backend"])

        # Create issue with backend label
        issue = service.create_issue("Backend task", labels=["backend"])

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            runner.invoke(cli, ["launch", issue.id])

        # Verify context file includes hint
        context_files = list((weaver_dir / ".weaver" / "launches" / "logs").glob("*-context.md"))
        assert len(context_files) == 1
        context_content = context_files[0].read_text()
        assert "Backend implementation guide" in context_content
        assert "Relevant Hints" in context_content

    def test_includes_dependencies_in_context(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)

        # Create dependency chain
        blocker = service.create_issue("Blocker task")
        issue = service.create_issue("Main task", blocked_by=[blocker.id])

        mock_result = Mock()
        mock_result.returncode = 0

        with patch("weaver.cli.subprocess.run", return_value=mock_result):
            runner.invoke(cli, ["launch", issue.id])

        # Verify context file includes dependencies
        context_files = list((weaver_dir / ".weaver" / "launches" / "logs").glob("*-context.md"))
        assert len(context_files) == 1
        context_content = context_files[0].read_text()
        assert "Dependencies (Blockers)" in context_content
        assert "Blocker task" in context_content
        assert blocker.id in context_content


    def test_not_in_weaver_project(self, runner: CliRunner, tmp_path: Path):
        with runner.isolated_filesystem(temp_dir=tmp_path):
            result = runner.invoke(cli, ["launch", "wv-test"])
            assert result.exit_code != 0
            assert "Not in a weaver project" in result.output
