"""CLI interface for Weaver issue tracker."""

import getpass
import subprocess
import sys
from pathlib import Path

import click
from rich.console import Console
from rich.table import Table

from weaver.models import Issue, IssueType, Status
from weaver.service import DependencyError, IssueNotFoundError, IssueService
from weaver.storage import MarkdownStorage

console = Console()


def find_weaver_root() -> Path | None:
    """Walk up from cwd to find .weaver directory."""
    path = Path.cwd()
    while path != path.parent:
        if (path / ".weaver").is_dir():
            return path / ".weaver"
        path = path.parent
    return None


def get_service(ctx: click.Context) -> IssueService:
    """Get service from context."""
    return ctx.obj["service"]


@click.group(invoke_without_command=True)
@click.pass_context
def cli(ctx: click.Context) -> None:
    """Weaver - Issue tracking for AI coding agents."""
    ctx.ensure_object(dict)
    root = find_weaver_root()
    if root is None and ctx.invoked_subcommand not in ("init", None):
        raise click.ClickException("Not in a weaver project. Run 'weaver init' first.")
    ctx.obj["service"] = IssueService(MarkdownStorage(root)) if root else None

    if ctx.invoked_subcommand is None:
        console.print(README_TEXT)


README_TEXT = """
[bold cyan]Weaver[/bold cyan] - Issue tracking for AI coding agents

[bold]Quick Start[/bold]
  weaver init                         Initialize a new weaver project
  weaver create "Title" [options]     Create a new issue
  weaver list                         List all issues
  weaver ready                        Show unblocked issues ready for work
  weaver show ID                      Show issue details
  weaver start ID                     Mark issue as in_progress
  weaver close ID                     Close an issue

[bold]Create Options[/bold]
  -t, --type TYPE       Issue type: task, bug, feature, epic, chore (default: task)
  -p, --priority 0-4    Priority level, 0=critical, 4=low (default: 2)
  -l, --label LABEL     Add label (repeatable)
  -b, --blocked-by ID   Block by another issue (repeatable)
  --parent ID           Parent epic
  -d, --description     Issue description
  -f, --file PATH       Read description from file (use '-' for stdin)

[bold]List/Filter Options[/bold]
  -s, --status STATUS   Filter: open, in_progress, blocked, closed
  -l, --label LABEL     Filter by label
  -t, --type TYPE       Filter by type
  -n, --limit N         Max results (ready command only)

[bold]Dependencies[/bold]
  weaver dep add CHILD PARENT   CHILD is blocked by PARENT
  weaver dep rm CHILD PARENT    Remove dependency

[bold]Sync (Multi-Instance)[/bold]
  weaver sync                   Sync issues to weaver-<username> branch
  weaver sync --branch NAME     Sync to a specific branch
  weaver sync --push            Push changes after sync
  weaver sync --pull            Pull changes before sync

[bold]Writing Good Issues[/bold]
  Structure issues to help AI agents (and humans) understand the work:

  [bold]Goal[/bold]: 1-2 sentence description of what to accomplish.

  [bold]Exit Conditions[/bold]: Concrete, verifiable criteria indicating completion.

  [bold]Related Code[/bold]: File paths, function names, or modules involved.

  [bold]Context[/bold] (optional): Background info, constraints, or design decisions.

  Example:
    ---
    id: wv-a1b2
    title: Fix token refresh race condition
    type: bug
    status: open
    priority: 1
    labels: [auth, backend]
    ---

    **Goal**: Prevent concurrent API requests from triggering multiple refreshes.

    **Exit Conditions**:
    - [ ] Only one refresh request occurs when token expires
    - [ ] Concurrent requests wait for the single refresh to complete
    - [ ] Tests cover the race condition scenario

    **Related Code**:
    - src/auth/token_manager.py: refresh_token(), get_valid_token()
    - tests/test_auth.py

    **Context**: Users report 401 errors when multiple tabs are open.

[bold]Example Workflow[/bold]
  weaver create "Add user auth" -t feature -p 1 -l backend
  weaver create "Add login endpoint" -t task -b wv-xxxx
  cat issue.md | weaver create "Complex feature" -f -
  weaver ready                  # Shows unblocked issues
  weaver start wv-yyyy          # Start working on an issue
  weaver close wv-yyyy          # Complete the issue
""".strip()


@cli.command()
def readme() -> None:
    """Show a quick reference guide for using weaver."""
    console.print(README_TEXT)


@cli.command()
def init() -> None:
    """Initialize a new weaver project."""
    root = Path.cwd() / ".weaver"
    if root.exists():
        console.print("[yellow]Weaver already initialized[/yellow]")
        return
    storage = MarkdownStorage(root)
    storage.ensure_initialized()

    # Add .weaver/ to .gitignore
    gitignore_path = Path.cwd() / ".gitignore"
    weaver_ignore_entry = ".weaver/"
    if gitignore_path.exists():
        gitignore_content = gitignore_path.read_text()
        if weaver_ignore_entry not in gitignore_content:
            with gitignore_path.open("a") as f:
                if not gitignore_content.endswith("\n"):
                    f.write("\n")
                f.write(f"{weaver_ignore_entry}\n")
            console.print("Added .weaver/ to .gitignore")
    else:
        gitignore_path.write_text(f"{weaver_ignore_entry}\n")
        console.print("Created .gitignore with .weaver/")

    console.print(f"Initialized weaver in {root}")


@cli.command()
@click.argument("title")
@click.option(
    "-t",
    "--type",
    "issue_type",
    type=click.Choice(["task", "bug", "feature", "epic", "chore"]),
    default="task",
    help="Issue type",
)
@click.option(
    "-p",
    "--priority",
    type=click.IntRange(0, 4),
    default=2,
    help="Priority (0=critical, 4=low)",
)
@click.option("-l", "--label", "labels", multiple=True, help="Add label (repeatable)")
@click.option(
    "-b", "--blocked-by", "blocked_by", multiple=True, help="Block by ID (repeatable)"
)
@click.option("--parent", help="Parent epic ID")
@click.option("-d", "--description", default="", help="Issue description")
@click.option(
    "-f",
    "--file",
    "file_path",
    type=click.Path(exists=False),
    help="Read description from file (use '-' for stdin)",
)
@click.pass_context
def create(
    ctx: click.Context,
    title: str,
    issue_type: str,
    priority: int,
    labels: tuple[str, ...],
    blocked_by: tuple[str, ...],
    parent: str | None,
    description: str,
    file_path: str | None,
) -> None:
    """Create a new issue.

    The description can be provided via -d/--description, or read from a file
    with -f/--file. Use '-f -' to read from stdin.
    """
    service = get_service(ctx)

    # File/stdin takes precedence over -d flag
    if file_path:
        if file_path == "-":
            description = sys.stdin.read()
        else:
            path = Path(file_path)
            if not path.exists():
                raise click.ClickException(f"File not found: {file_path}")
            description = path.read_text()

    try:
        issue = service.create_issue(
            title=title,
            type=IssueType(issue_type),
            priority=priority,
            description=description,
            labels=list(labels),
            blocked_by=list(blocked_by),
            parent=parent,
        )
        console.print(f"Created [cyan]{issue.id}[/cyan]: {issue.title}")
    except DependencyError as e:
        raise click.ClickException(str(e))


@cli.command()
@click.argument("issue_id")
@click.pass_context
def show(ctx: click.Context, issue_id: str) -> None:
    """Show issue details."""
    service = get_service(ctx)
    issue = service.get_issue(issue_id)
    if issue is None:
        raise click.ClickException(f"Issue {issue_id} not found")

    console.print(f"[bold cyan]{issue.id}[/bold cyan]: {issue.title}")
    console.print(
        f"Status: {issue.status.value}  Priority: P{issue.priority}  Type: {issue.type.value}"
    )
    if issue.labels:
        console.print(f"Labels: {', '.join(issue.labels)}")
    if issue.blocked_by:
        console.print(f"Blocked by: {', '.join(issue.blocked_by)}")
    if issue.parent:
        console.print(f"Parent: {issue.parent}")
    if issue.description:
        console.print(f"\n{issue.description}")
    if issue.design_notes:
        console.print(f"\n[bold]Design Notes[/bold]\n{issue.design_notes}")
    if issue.acceptance_criteria:
        console.print("\n[bold]Acceptance Criteria[/bold]")
        for criterion in issue.acceptance_criteria:
            console.print(f"  - [ ] {criterion}")


@cli.command("list")
@click.option(
    "-s",
    "--status",
    type=click.Choice(["open", "in_progress", "blocked", "closed"]),
    help="Filter by status",
)
@click.option("-l", "--label", "labels", multiple=True, help="Filter by label")
@click.option(
    "-t",
    "--type",
    "issue_type",
    type=click.Choice(["task", "bug", "feature", "epic", "chore"]),
    help="Filter by type",
)
@click.pass_context
def list_issues(
    ctx: click.Context,
    status: str | None,
    labels: tuple[str, ...],
    issue_type: str | None,
) -> None:
    """List issues with optional filters."""
    service = get_service(ctx)
    issues = service.list_issues(
        status=Status(status) if status else None,
        labels=list(labels) if labels else None,
        type=IssueType(issue_type) if issue_type else None,
    )
    if not issues:
        console.print("No issues found.")
        return
    _print_issue_table(issues)


@cli.command()
@click.option(
    "-l",
    "--label",
    "labels",
    multiple=True,
    help="Filter by label (can specify multiple)",
)
@click.option(
    "-t",
    "--type",
    "issue_type",
    type=click.Choice(["task", "bug", "feature", "epic", "chore"]),
    help="Filter by issue type",
)
@click.option("-n", "--limit", type=int, help="Max number of issues to show")
@click.pass_context
def ready(
    ctx: click.Context,
    labels: tuple[str, ...],
    issue_type: str | None,
    limit: int | None,
) -> None:
    """List unblocked issues ready for work."""
    service = get_service(ctx)
    issues = service.get_ready_issues(
        labels=list(labels) if labels else None,
        type=IssueType(issue_type) if issue_type else None,
        limit=limit,
    )
    if not issues:
        console.print("No ready issues found.")
        return
    _print_issue_table(issues)


@cli.command()
@click.argument("issue_id")
@click.pass_context
def start(ctx: click.Context, issue_id: str) -> None:
    """Mark an issue as in progress."""
    service = get_service(ctx)
    try:
        issue = service.start_issue(issue_id)
        console.print(f"Started [cyan]{issue.id}[/cyan]: {issue.title}")
    except IssueNotFoundError:
        raise click.ClickException(f"Issue {issue_id} not found")


@cli.command()
@click.argument("issue_id")
@click.pass_context
def close(ctx: click.Context, issue_id: str) -> None:
    """Close an issue."""
    service = get_service(ctx)
    try:
        issue = service.close_issue(issue_id)
        console.print(f"Closed [cyan]{issue.id}[/cyan]: {issue.title}")
    except IssueNotFoundError:
        raise click.ClickException(f"Issue {issue_id} not found")


@cli.group()
def dep() -> None:
    """Manage issue dependencies."""
    pass


@dep.command("add")
@click.argument("issue_id")
@click.argument("blocked_by_id")
@click.pass_context
def dep_add(ctx: click.Context, issue_id: str, blocked_by_id: str) -> None:
    """Mark ISSUE_ID as blocked by BLOCKED_BY_ID."""
    service = get_service(ctx)
    try:
        service.add_dependency(issue_id, blocked_by_id)
        console.print(f"[cyan]{issue_id}[/cyan] is now blocked by [cyan]{blocked_by_id}[/cyan]")
    except IssueNotFoundError as e:
        raise click.ClickException(f"Issue not found: {e.issue_id}")
    except DependencyError as e:
        raise click.ClickException(str(e))


@dep.command("rm")
@click.argument("issue_id")
@click.argument("blocked_by_id")
@click.pass_context
def dep_rm(ctx: click.Context, issue_id: str, blocked_by_id: str) -> None:
    """Remove dependency: ISSUE_ID no longer blocked by BLOCKED_BY_ID."""
    service = get_service(ctx)
    try:
        service.remove_dependency(issue_id, blocked_by_id)
        console.print(
            f"[cyan]{issue_id}[/cyan] is no longer blocked by [cyan]{blocked_by_id}[/cyan]"
        )
    except IssueNotFoundError as e:
        raise click.ClickException(f"Issue not found: {e.issue_id}")


def _print_issue_table(issues: list[Issue]) -> None:
    """Print issues as a formatted table."""
    table = Table()
    table.add_column("ID", style="cyan")
    table.add_column("P", justify="center")
    table.add_column("Status")
    table.add_column("Type")
    table.add_column("Title")
    table.add_column("Labels")

    for issue in issues:
        table.add_row(
            issue.id,
            str(issue.priority),
            issue.status.value,
            issue.type.value,
            issue.title[:50],
            ", ".join(issue.labels),
        )
    console.print(table)


def _get_default_sync_branch() -> str:
    """Get the default sync branch name based on username."""
    return f"weaver-{getpass.getuser()}"


def _run_git(args: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    """Run a git command and return the result."""
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )


def _flip_gitignore_for_sync(project_root: Path) -> bool:
    """
    Flip .gitignore from ignoring all of .weaver/ to only ignoring index.yml.

    Returns True if changes were made.
    """
    gitignore_path = project_root / ".gitignore"
    if not gitignore_path.exists():
        gitignore_path.write_text(".weaver/index.yml\n")
        return True

    content = gitignore_path.read_text()
    lines = content.splitlines()
    new_lines = []
    changed = False

    for line in lines:
        stripped = line.strip()
        if stripped == ".weaver/" or stripped == ".weaver":
            new_lines.append(".weaver/index.yml")
            changed = True
        else:
            new_lines.append(line)

    if ".weaver/index.yml" not in [l.strip() for l in new_lines]:
        new_lines.append(".weaver/index.yml")
        changed = True

    if changed:
        gitignore_path.write_text("\n".join(new_lines) + "\n")

    return changed


def _is_git_repo(path: Path) -> bool:
    """Check if path is inside a git repository."""
    result = _run_git(["rev-parse", "--git-dir"], cwd=path)
    return result.returncode == 0


def _branch_exists(branch: str, cwd: Path) -> bool:
    """Check if a branch exists locally or remotely."""
    local = _run_git(["show-ref", "--verify", f"refs/heads/{branch}"], cwd=cwd)
    if local.returncode == 0:
        return True
    remote = _run_git(["show-ref", "--verify", f"refs/remotes/origin/{branch}"], cwd=cwd)
    return remote.returncode == 0


@cli.command()
@click.option(
    "-b",
    "--branch",
    default=None,
    help="Branch to sync with (default: weaver-<username>)",
)
@click.option("--push", "do_push", is_flag=True, help="Push changes after sync")
@click.option("--pull", "do_pull", is_flag=True, help="Pull changes before sync")
def sync(branch: str | None, do_push: bool, do_pull: bool) -> None:
    """Synchronize issues with a git branch.

    By default syncs to weaver-<username> branch. This command flips
    the .gitignore to track .weaver/issues/ while keeping index.yml ignored.
    """
    weaver_root = find_weaver_root()
    if weaver_root is None:
        raise click.ClickException("Not in a weaver project. Run 'weaver init' first.")

    project_root = weaver_root.parent
    branch = branch or _get_default_sync_branch()

    if not _is_git_repo(project_root):
        raise click.ClickException("Not in a git repository. Initialize git first.")

    # Flip gitignore to enable syncing
    if _flip_gitignore_for_sync(project_root):
        console.print("Updated .gitignore to track .weaver/issues/")

    issues_dir = weaver_root / "issues"

    if do_pull:
        # Fetch and merge from the sync branch
        console.print(f"Pulling from [cyan]{branch}[/cyan]...")
        _run_git(["fetch", "origin", branch], cwd=project_root)

        if _branch_exists(branch, project_root):
            # Checkout the issues from the remote branch
            result = _run_git(
                ["checkout", f"origin/{branch}", "--", ".weaver/issues/"],
                cwd=project_root,
            )
            if result.returncode == 0:
                console.print(f"Pulled issues from [cyan]{branch}[/cyan]")
            else:
                console.print(f"[yellow]No issues found on {branch}[/yellow]")
        else:
            console.print(f"[yellow]Branch {branch} does not exist yet[/yellow]")

    if do_push:
        # Get current branch
        current_branch_result = _run_git(
            ["rev-parse", "--abbrev-ref", "HEAD"], cwd=project_root
        )
        current_branch = current_branch_result.stdout.strip()

        # Create or checkout sync branch
        if not _branch_exists(branch, project_root):
            console.print(f"Creating branch [cyan]{branch}[/cyan]...")
            _run_git(["checkout", "-b", branch], cwd=project_root)
        else:
            _run_git(["checkout", branch], cwd=project_root)
            # Pull latest if remote exists
            _run_git(["pull", "origin", branch, "--rebase"], cwd=project_root)

        # Stage and commit issues
        if issues_dir.exists():
            _run_git(["add", ".weaver/issues/"], cwd=project_root)
            _run_git(["add", ".gitignore"], cwd=project_root)

            # Check if there are changes to commit
            status_result = _run_git(["diff", "--cached", "--quiet"], cwd=project_root)
            if status_result.returncode != 0:
                _run_git(
                    ["commit", "-m", "Sync weaver issues"],
                    cwd=project_root,
                )
                console.print("Committed issue changes")

            # Push to remote
            push_result = _run_git(
                ["push", "-u", "origin", branch],
                cwd=project_root,
            )
            if push_result.returncode == 0:
                console.print(f"Pushed to [cyan]origin/{branch}[/cyan]")
            else:
                console.print(f"[red]Failed to push:[/red] {push_result.stderr}")

        # Return to original branch
        _run_git(["checkout", current_branch], cwd=project_root)

    if not do_push and not do_pull:
        console.print(f"Sync branch: [cyan]{branch}[/cyan]")
        console.print("Use --pull to pull issues, --push to push issues")

        # Show status
        if issues_dir.exists():
            issue_count = len(list(issues_dir.glob("*.md")))
            console.print(f"Local issues: {issue_count}")


if __name__ == "__main__":
    cli()
