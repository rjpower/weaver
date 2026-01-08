"""CLI interface for Weaver issue tracker."""

import getpass
import subprocess
import sys
from importlib.resources import files
from pathlib import Path

import click
from rich.console import Console
from rich.markdown import Markdown
from rich.table import Table

from weaver.models import Issue, Status
from weaver.service import DependencyError, IssueNotFoundError, IssueService, HintService, WorkflowService
from weaver.storage import MarkdownStorage, HintStorage, WorkflowStorage

console = Console()


def get_readme_content() -> str:
    """Load README.md content from package."""
    readme_path = files("weaver").joinpath("README.md")
    return readme_path.read_text()


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
        console.print(Markdown(get_readme_content()))


@cli.command()
def readme() -> None:
    """Show a quick reference guide for using weaver."""
    console.print(Markdown(get_readme_content()))


@cli.command()
def init() -> None:
    """Initialize a new weaver project."""
    root = Path.cwd() / ".weaver"
    if root.exists():
        console.print("[yellow]Weaver already initialized[/yellow]")
        return
    storage = MarkdownStorage(root)
    storage.ensure_initialized()
    hint_storage = HintStorage(root)
    hint_storage.ensure_initialized()

    from weaver.storage import WorkflowStorage, LaunchStorage
    workflow_storage = WorkflowStorage(root)
    workflow_storage.ensure_initialized()
    launch_storage = LaunchStorage(root)
    launch_storage.ensure_initialized()

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
@click.option("--fetch-deps", is_flag=True, help="Show transitive dependencies in topological order")
@click.pass_context
def show(ctx: click.Context, issue_id: str, fetch_deps: bool) -> None:
    """Show issue details."""
    service = get_service(ctx)

    if fetch_deps:
        try:
            issue, dependencies = service.get_issue_with_dependencies(issue_id)
        except IssueNotFoundError:
            raise click.ClickException(f"Issue {issue_id} not found")

        # Display dependencies first
        if dependencies:
            console.print("[bold]Dependencies (topological order - deepest first):[/bold]\n")

            for dep in dependencies:
                console.print(f"[bold cyan]{dep.id}[/bold cyan]: {dep.title}")
                console.print(f"Status: {dep.status.value}  Priority: P{dep.priority}")

                # Combine description and design_notes for truncation
                content = dep.description
                if dep.design_notes:
                    content += "\n\n" + dep.design_notes

                # Use truncate_content utility
                if content:
                    from weaver.utils import truncate_content
                    truncated, was_truncated = truncate_content(content, max_words=200)
                    console.print(f"\n{truncated}\n")

                    if was_truncated:
                        console.print(f"[dim]Use 'weaver show {dep.id}' to see complete content[/dim]\n")

                console.print("─" * 60)
                console.print()

        # Display main issue
        console.print("[bold]Main Issue:[/bold]\n")
    else:
        issue = service.get_issue(issue_id)
        if issue is None:
            raise click.ClickException(f"Issue {issue_id} not found")

    # Display issue details
    console.print(f"[bold cyan]{issue.id}[/bold cyan]: {issue.title}")
    console.print(
        f"Status: {issue.status.value}  Priority: P{issue.priority}"
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
    "-a",
    "--all",
    "show_all",
    is_flag=True,
    help="Show all issues including closed ones",
)
@click.pass_context
def list_issues(
    ctx: click.Context,
    status: str | None,
    labels: tuple[str, ...],
    show_all: bool,
) -> None:
    """List issues with optional filters. By default, closed issues are hidden."""
    service = get_service(ctx)
    issues = service.list_issues(
        status=Status(status) if status else None,
        labels=list(labels) if labels else None,
        exclude_closed=not show_all and status is None,
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
@click.option("-n", "--limit", type=int, help="Max number of issues to show")
@click.pass_context
def ready(
    ctx: click.Context,
    labels: tuple[str, ...],
    limit: int | None,
) -> None:
    """List unblocked issues ready for work."""
    service = get_service(ctx)
    issues = service.get_ready_issues(
        labels=list(labels) if labels else None,
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
    table.add_column("Title")
    table.add_column("Labels")
    table.add_column("Blocked By")

    for issue in issues:
        table.add_row(
            issue.id,
            str(issue.priority),
            issue.status.value,
            issue.title[:50],
            ", ".join(issue.labels),
            ", ".join(issue.blocked_by) if issue.blocked_by else "",
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


@cli.group()
def workflow() -> None:
    """Manage workflow templates."""
    pass


@workflow.command("create")
@click.argument("name")
@click.option(
    "-f",
    "--file",
    "file_path",
    type=click.Path(exists=False),
    help="Read workflow YAML from file (use '-' for stdin)",
)
@click.pass_context
def workflow_create(ctx: click.Context, name: str, file_path: str | None) -> None:
    """Create or update a workflow template."""
    workflow_service = get_workflow_service(ctx)

    if not file_path:
        raise click.ClickException("Workflow YAML required. Use -f - to read from stdin.")

    if file_path == "-":
        yaml_content = sys.stdin.read()
    else:
        path = Path(file_path)
        if not path.exists():
            raise click.ClickException(f"File not found: {file_path}")
        yaml_content = path.read_text()

    try:
        workflow = workflow_service.create_or_update_workflow(yaml_content)
        console.print(f"Created workflow [cyan]{workflow.name}[/cyan] ({workflow.id})")
        console.print(f"Steps: {len(workflow.steps)}")
    except Exception as e:
        raise click.ClickException(f"Failed to parse workflow: {e}")


@workflow.command("execute")
@click.argument("workflow_name")
@click.option("--label", help="Additional label prefix for created issues")
@click.pass_context
def workflow_execute(ctx: click.Context, workflow_name: str, label: str | None) -> None:
    """Execute a workflow, creating all its issues."""
    workflow_service = get_workflow_service(ctx)

    try:
        issues = workflow_service.execute_workflow(workflow_name, label)
        console.print(f"Created {len(issues)} issues from workflow [cyan]{workflow_name}[/cyan]:")

        for issue in issues:
            console.print(f"  [cyan]{issue.id}[/cyan]: {issue.title}")

    except ValueError as e:
        raise click.ClickException(str(e))


@workflow.command("show")
@click.argument("workflow_name")
@click.pass_context
def workflow_show(ctx: click.Context, workflow_name: str) -> None:
    """Show workflow details."""
    workflow_service = get_workflow_service(ctx)
    workflow = workflow_service.get_workflow(workflow_name)

    if not workflow:
        raise click.ClickException(f"Workflow not found: {workflow_name}")

    console.print(f"[bold cyan]{workflow.name}[/bold cyan] ({workflow.id})")
    if workflow.description:
        console.print(f"{workflow.description}\n")

    console.print(f"[bold]Steps ({len(workflow.steps)}):[/bold]")
    for i, step in enumerate(workflow.steps, 1):
        console.print(f"\n{i}. [cyan]{step.title}[/cyan]")
        console.print(f"   Priority: P{step.priority}")
        if step.depends_on:
            console.print(f"   Depends on: {', '.join(step.depends_on)}")
        if step.labels:
            console.print(f"   Labels: {', '.join(step.labels)}")


@workflow.command("list")
@click.pass_context
def workflow_list(ctx: click.Context) -> None:
    """List all workflows."""
    workflow_service = get_workflow_service(ctx)
    workflows = workflow_service.list_workflows()

    if not workflows:
        console.print("No workflows found.")
        return

    table = Table()
    table.add_column("Name", style="cyan")
    table.add_column("ID")
    table.add_column("Steps")
    table.add_column("Description")

    for wf in workflows:
        table.add_row(
            wf.name,
            wf.id,
            str(len(wf.steps)),
            wf.description[:50] if wf.description else "",
        )

    console.print(table)


def get_workflow_service(ctx: click.Context) -> WorkflowService:
    """Get or create WorkflowService from context."""
    if "workflow_service" not in ctx.obj:
        root = find_weaver_root()
        if not root:
            raise click.ClickException("Not in a weaver project.")
        service = get_service(ctx)
        workflow_storage = WorkflowStorage(root)
        workflow_storage.ensure_initialized()
        ctx.obj["workflow_service"] = WorkflowService(workflow_storage, service)
    return ctx.obj["workflow_service"]


def get_hint_service(ctx: click.Context) -> HintService:
    """Get or create HintService from context."""
    if "hint_service" not in ctx.obj:
        root = find_weaver_root()
        if not root:
            raise click.ClickException("Not in a weaver project.")
        hint_storage = HintStorage(root)
        hint_storage.ensure_initialized()
        ctx.obj["hint_service"] = HintService(hint_storage)
    return ctx.obj["hint_service"]


@cli.group()
def hint() -> None:
    """Manage repository knowledge hints."""
    pass


@hint.command("add")
@click.argument("title")
@click.option("-l", "--label", "labels", multiple=True, help="Add label (repeatable)")
@click.option(
    "-f",
    "--file",
    "file_path",
    type=click.Path(exists=False),
    help="Read content from file (use '-' for stdin)",
)
@click.pass_context
def hint_add(ctx: click.Context, title: str, labels: tuple[str, ...], file_path: str | None) -> None:
    """Add or update a hint. Use -f - to read from stdin/HEREDOC."""
    hint_service = get_hint_service(ctx)

    if not file_path:
        raise click.ClickException("Content required. Use -f - to read from stdin.")

    if file_path == "-":
        content = sys.stdin.read()
    else:
        path = Path(file_path)
        if not path.exists():
            raise click.ClickException(f"File not found: {file_path}")
        content = path.read_text()

    existing = hint_service.get_hint(title)
    hint = hint_service.create_or_update_hint(title, content, list(labels))
    action = "Updated" if existing else "Created"
    console.print(f"{action} hint [cyan]{hint.title}[/cyan] ({hint.id})")


@hint.command("show")
@click.argument("title_or_id")
@click.pass_context
def hint_show(ctx: click.Context, title_or_id: str) -> None:
    """Show a hint by title or ID."""
    hint_service = get_hint_service(ctx)
    hint_obj = hint_service.get_hint(title_or_id)

    if not hint_obj:
        raise click.ClickException(f"Hint not found: {title_or_id}")

    console.print(f"[bold cyan]{hint_obj.title}[/bold cyan] ({hint_obj.id})")
    if hint_obj.labels:
        console.print(f"Labels: {', '.join(hint_obj.labels)}")
    console.print(f"\n{hint_obj.content}")


@hint.command("list")
@click.pass_context
def hint_list(ctx: click.Context) -> None:
    """List all hints."""
    hint_service = get_hint_service(ctx)
    hints = hint_service.list_hints()

    if not hints:
        console.print("No hints found.")
        return

    table = Table()
    table.add_column("Title", style="cyan")
    table.add_column("ID")
    table.add_column("Labels")

    for hint_obj in hints:
        table.add_row(hint_obj.title, hint_obj.id, ", ".join(hint_obj.labels))

    console.print(table)


@hint.command("search")
@click.argument("query")
@click.pass_context
def hint_search(ctx: click.Context, query: str) -> None:
    """Search hints by content."""
    hint_service = get_hint_service(ctx)
    hints = hint_service.search_hints(query)

    if not hints:
        console.print(f"No hints found matching '{query}'")
        return

    from weaver.utils import truncate_content
    for hint_obj in hints:
        console.print(f"[cyan]{hint_obj.title}[/cyan] ({hint_obj.id})")
        preview, _ = truncate_content(hint_obj.content, max_words=50)
        console.print(f"  {preview}\n")


@cli.command()
@click.argument("issue_id")
@click.option(
    "--model",
    type=click.Choice(["sonnet", "opus", "flash"]),
    default="sonnet",
    help="Claude model to use",
)
@click.option(
    "--follow",
    is_flag=True,
    help="Stream logs to console in real-time while agent runs",
)
@click.pass_context
def launch(ctx: click.Context, issue_id: str, model: str, follow: bool) -> None:
    """Launch an AI agent to work on a task.

    This spawns a Claude agent using the Agent SDK that will attempt to complete
    the task autonomously. The agent receives the task context, relevant hints,
    and dependency information.
    """
    service = get_service(ctx)
    hint_service = get_hint_service(ctx)

    # Validate issue exists
    issue = service.get_issue(issue_id)
    if not issue:
        raise click.ClickException(f"Issue {issue_id} not found")

    # Map model name to enum
    from weaver.models import AgentModel
    model_map = {
        "sonnet": AgentModel.SONNET,
        "opus": AgentModel.OPUS,
        "flash": AgentModel.FLASH,
    }
    agent_model = model_map[model]

    # Create launch service
    root = find_weaver_root()
    from weaver.storage import LaunchStorage
    from weaver.service import LaunchService

    launch_storage = LaunchStorage(root)
    launch_storage.ensure_initialized()
    launch_service = LaunchService(service, launch_storage, hint_service)

    # Prepare and launch
    console.print(f"Launching {model} agent on [cyan]{issue_id}[/cyan]: {issue.title}")
    launch = launch_service.launch_agent(issue_id, agent_model)

    # Get context file and log file paths
    from pathlib import Path
    context_file = Path(launch.log_file).parent / f"{launch.id}-context.md"
    log_file = Path(launch.log_file)

    console.print(f"Model: {agent_model.value}")
    console.print(f"Context: {context_file}")
    console.print(f"Logs: {log_file}")
    if follow:
        console.print(f"[bold]Streaming logs to console...[/bold]")

    # Read the context file to get the prompt
    with open(context_file, "r") as f:
        prompt = f.read()

    # Run the agent using the SDK
    import anyio
    from claude_agent_sdk import query, ClaudeAgentOptions
    from datetime import datetime

    async def run_agent():
        """Run the agent and stream output to log file."""
        # Use the parent directory of .weaver as the working directory
        project_root = root.parent if root else Path.cwd()

        options = ClaudeAgentOptions(
            model=agent_model.value,
            cwd=str(project_root),
            permission_mode="bypassPermissions",
        )

        with open(log_file, "w") as log:
            async for message in query(prompt=prompt, options=options):
                # Write message as JSON to log file for proper parsing
                from weaver.utils import serialize_message
                log.write(serialize_message(message))
                log.write("\n")
                log.flush()
                # Stream to console if --follow is enabled
                if follow:
                    from weaver.utils import format_agent_message
                    format_agent_message(message, console)
            return 0

    # Unset ANTHROPIC_API_KEY to use Claude Code OAuth credentials
    import os
    from contextlib import contextmanager

    @contextmanager
    def without_api_key():
        api_key = os.environ.pop("ANTHROPIC_API_KEY", None)
        auth_token = os.environ.pop("ANTHROPIC_AUTH_TOKEN", None)

        if api_key:
            console.print("[dim]Temporarily unsetting ANTHROPIC_API_KEY to use OAuth[/dim]")
        if auth_token:
            console.print("[dim]Temporarily unsetting ANTHROPIC_AUTH_TOKEN to use OAuth[/dim]")

        try:
            yield
        finally:
            if api_key:
                os.environ["ANTHROPIC_API_KEY"] = api_key
            if auth_token:
                os.environ["ANTHROPIC_AUTH_TOKEN"] = auth_token

    with without_api_key():
        exit_code = anyio.run(run_agent)

    launch.completed_at = datetime.now()
    launch.exit_code = exit_code
    launch_storage.write_launch(launch)

    if exit_code == 0:
        console.print(f"[green]Agent completed successfully[/green]")
    else:
        console.print(f"[red]Agent exited with code {exit_code}[/red]")
        console.print(f"Check logs: {log_file}")

    # Summarize the conversation log using Haiku
    from weaver.utils import summarize_conversation_log
    summary = summarize_conversation_log(log_file)
    if summary:
        console.print(f"\n[bold]Summary:[/bold]\n{summary}")


if __name__ == "__main__":
    cli()
