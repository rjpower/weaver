"""Tests for weaver.cli."""

import os
from pathlib import Path

import pytest
from click.testing import CliRunner

from weaver.cli import cli
from weaver.models import IssueType, Status
from weaver.service import IssueService
from weaver.storage import MarkdownStorage


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

    def test_with_options(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        result = runner.invoke(
            cli,
            [
                "create",
                "Feature",
                "-t",
                "feature",
                "-p",
                "0",
                "-l",
                "backend",
                "-l",
                "api",
            ],
        )

        assert result.exit_code == 0
        issues = service.list_issues()
        assert issues[0].type == IssueType.FEATURE
        assert issues[0].priority == 0
        assert set(issues[0].labels) == {"backend", "api"}

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

    def test_with_file_description(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService
    ):
        os.chdir(weaver_dir)
        desc_file = weaver_dir / "description.md"
        desc_file.write_text("This is a detailed description\nwith multiple lines.")

        result = runner.invoke(
            cli, ["create", "Issue from file", "-f", str(desc_file)]
        )

        assert result.exit_code == 0
        issues = service.list_issues()
        assert issues[0].description == "This is a detailed description\nwith multiple lines."

    def test_with_stdin_description(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService
    ):
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
        result = runner.invoke(
            cli, ["create", "Test", "-f", "/nonexistent/path.md"]
        )

        assert result.exit_code != 0
        assert "File not found" in result.output

    def test_file_overrides_description_flag(
        self, runner: CliRunner, weaver_dir: Path, service: IssueService
    ):
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
            type=IssueType.FEATURE,
            priority=1,
            labels=["backend"],
        )

        result = runner.invoke(cli, ["show", issue.id])

        assert result.exit_code == 0
        assert issue.id in result.output
        assert "Test issue" in result.output
        assert "feature" in result.output
        assert "P1" in result.output
        assert "backend" in result.output

    def test_not_found(self, runner: CliRunner, weaver_dir: Path):
        os.chdir(weaver_dir)
        result = runner.invoke(cli, ["show", "wv-nonexistent"])

        assert result.exit_code != 0
        assert "not found" in result.output


class TestList:
    def test_lists_all(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        service.create_issue("Issue 1")
        service.create_issue("Issue 2")

        result = runner.invoke(cli, ["list"])

        assert result.exit_code == 0
        assert "Issue 1" in result.output
        assert "Issue 2" in result.output

    def test_filters_by_status(self, runner: CliRunner, weaver_dir: Path, service: IssueService):
        os.chdir(weaver_dir)
        open_issue = service.create_issue("Open")
        closed_issue = service.create_issue("Closed")
        service.close_issue(closed_issue.id)

        result = runner.invoke(cli, ["list", "-s", "open"])

        assert result.exit_code == 0
        assert "Open" in result.output
        assert "Closed" not in result.output

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
        assert "Quick Start" in result.output
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
