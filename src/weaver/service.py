"""Business logic service for Weaver issue tracker."""

from datetime import datetime

import yaml

from weaver.graph import DependencyGraph
from weaver.models import (
    AgentModel,
    Hint,
    Issue,
    IssueType,
    LaunchExecution,
    Status,
    Workflow,
    WorkflowStep,
    generate_id,
)
from weaver.storage import HintStorage, LaunchStorage, MarkdownStorage, WorkflowStorage


class IssueNotFoundError(Exception):
    """Raised when an issue is not found."""

    def __init__(self, issue_id: str):
        self.issue_id = issue_id
        super().__init__(f"Issue not found: {issue_id}")


class DependencyError(Exception):
    """Raised when a dependency operation fails."""

    pass


class IssueService:
    """Business logic layer for issue operations."""

    def __init__(self, storage: MarkdownStorage):
        self.storage = storage
        self._graph: DependencyGraph | None = None

    def _invalidate_graph(self) -> None:
        """Invalidate cached graph when issues change."""
        self._graph = None

    def _get_graph(self) -> DependencyGraph:
        """Get or build dependency graph."""
        if self._graph is None:
            issues = self.storage.read_all_issues()
            self._graph = DependencyGraph.build(issues)
        return self._graph

    def create_issue(
        self,
        title: str,
        type: IssueType = IssueType.TASK,
        priority: int = 2,
        description: str = "",
        labels: list[str] | None = None,
        blocked_by: list[str] | None = None,
        parent: str | None = None,
    ) -> Issue:
        """Create a new issue."""
        # Validate blocked_by references
        if blocked_by:
            for dep_id in blocked_by:
                if not self.validate_issue_exists(dep_id):
                    raise DependencyError(f"Cannot block by non-existent issue: {dep_id}")

        # Validate parent reference
        if parent and not self.validate_issue_exists(parent):
            raise DependencyError(f"Parent issue not found: {parent}")

        issue = Issue(
            id=generate_id(),
            title=title,
            type=type,
            priority=priority,
            description=description,
            labels=labels or [],
            blocked_by=blocked_by or [],
            parent=parent,
        )
        self.storage.write_issue(issue)
        self._invalidate_graph()
        return issue

    def get_issue(self, issue_id: str) -> Issue | None:
        """Get an issue by ID."""
        return self.storage.read_issue(issue_id)

    def update_issue(self, issue: Issue) -> None:
        """Update an existing issue."""
        issue.updated_at = datetime.now()
        self.storage.write_issue(issue)
        self._invalidate_graph()

    def close_issue(self, issue_id: str) -> Issue:
        """Close an issue."""
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        issue.status = Status.CLOSED
        issue.closed_at = datetime.now()
        self.update_issue(issue)
        return issue

    def start_issue(self, issue_id: str) -> Issue:
        """Mark an issue as in progress."""
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        issue.status = Status.IN_PROGRESS
        self.update_issue(issue)
        return issue

    def validate_issue_exists(self, issue_id: str) -> bool:
        """Check if an issue exists."""
        return self.storage.read_issue(issue_id) is not None

    def add_dependency(self, issue_id: str, blocked_by_id: str) -> None:
        """
        Add dependency: issue_id becomes blocked by blocked_by_id.

        Raises:
            IssueNotFoundError: If either issue doesn't exist.
            DependencyError: If adding the dependency would create a cycle.
        """
        # Validate both issues exist
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        if not self.validate_issue_exists(blocked_by_id):
            raise IssueNotFoundError(blocked_by_id)

        # Check for cycle
        graph = self._get_graph()
        if graph.detect_cycle(issue_id, blocked_by_id):
            raise DependencyError(
                f"Cannot add dependency: {issue_id} -> {blocked_by_id} would create a cycle"
            )

        # Add dependency if not already present
        if blocked_by_id not in issue.blocked_by:
            issue.blocked_by.append(blocked_by_id)
            self.update_issue(issue)

    def remove_dependency(self, issue_id: str, blocked_by_id: str) -> None:
        """
        Remove dependency: issue_id is no longer blocked by blocked_by_id.

        Raises:
            IssueNotFoundError: If the issue doesn't exist.
        """
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        if blocked_by_id in issue.blocked_by:
            issue.blocked_by.remove(blocked_by_id)
            self.update_issue(issue)

    def list_issues(
        self,
        status: Status | None = None,
        labels: list[str] | None = None,
        type: IssueType | None = None,
    ) -> list[Issue]:
        """List issues with optional filters."""
        issues = self.storage.read_all_issues()

        if status is not None:
            issues = [i for i in issues if i.status == status]
        if labels:
            label_set = set(labels)
            issues = [i for i in issues if label_set & set(i.labels)]
        if type is not None:
            issues = [i for i in issues if i.type == type]

        return sorted(issues, key=lambda i: (i.priority, i.created_at))

    def get_ready_issues(
        self,
        labels: list[str] | None = None,
        type: IssueType | None = None,
        limit: int | None = None,
    ) -> list[Issue]:
        """Get unblocked issues ready for work, with filters."""
        issues = self.storage.read_all_issues()
        open_issues = [i for i in issues if i.is_open()]

        graph = self._get_graph()
        ready = graph.get_unblocked(open_issues)

        # Apply filters
        if labels:
            label_set = set(labels)
            ready = [i for i in ready if label_set & set(i.labels)]
        if type is not None:
            ready = [i for i in ready if i.type == type]

        # Sort by priority, then creation date
        ready = sorted(ready, key=lambda i: (i.priority, i.created_at))

        if limit:
            ready = ready[:limit]

        return ready

    def get_issue_with_dependencies(self, issue_id: str) -> tuple[Issue, list[Issue]]:
        """Get issue and all its transitive dependencies in topological order.

        Returns:
            Tuple of (main_issue, list_of_dependencies_in_topo_order)

        Raises:
            IssueNotFoundError: If issue_id doesn't exist
        """
        issue = self.get_issue(issue_id)
        if issue is None:
            raise IssueNotFoundError(issue_id)

        graph = self._get_graph()
        dep_ids = graph.get_transitive_blockers(issue_id)

        dependencies = []
        for dep_id in dep_ids:
            dep_issue = self.get_issue(dep_id)
            if dep_issue is not None:
                dependencies.append(dep_issue)

        return (issue, dependencies)


class HintService:
    """Business logic for hints."""

    def __init__(self, storage: HintStorage):
        self.storage = storage

    def create_or_update_hint(
        self, title: str, content: str, labels: list[str] | None = None
    ) -> Hint:
        """Create new hint or update existing one with same title.

        If a hint with the same title exists (case-insensitive), update it:
        - Preserve the existing ID
        - Update content, labels, and updated_at timestamp

        Otherwise, create a new hint with a generated ID.
        """
        existing = self.storage.find_hint_by_title(title)

        if existing:
            existing.content = content
            existing.labels = labels or []
            existing.updated_at = datetime.now()
            self.storage.write_hint(existing)
            return existing

        hint = Hint(
            id=generate_id("wv-hint"),
            title=title.lower(),
            content=content,
            labels=labels or [],
        )
        self.storage.write_hint(hint)
        return hint

    def get_hint(self, title_or_id: str) -> Hint | None:
        """Get hint by title (case-insensitive) or ID.

        Try by ID first, then by title.
        """
        hint = self.storage.read_hint(title_or_id)
        if hint is not None:
            return hint
        return self.storage.find_hint_by_title(title_or_id)

    def list_hints(self) -> list[Hint]:
        """Return all hints sorted by title."""
        return self.storage.list_all_hints()

    def search_hints(self, query: str) -> list[Hint]:
        """Search hints by query in title and content."""
        return self.storage.search_hints(query)


class WorkflowService:
    """Service for workflow templates."""

    def __init__(self, workflow_storage: WorkflowStorage, issue_service: IssueService):
        self.storage = workflow_storage
        self.issue_service = issue_service

    def parse_workflow_yaml(self, yaml_content: str) -> Workflow:
        """Parse YAML workflow definition into Workflow object."""
        data = yaml.safe_load(yaml_content)

        name = data["name"]
        description = data.get("description", "")
        steps_data = data.get("steps", [])

        steps = []
        for step_dict in steps_data:
            step = WorkflowStep(
                title=step_dict["title"],
                type=IssueType(step_dict.get("type", "task")),
                priority=step_dict.get("priority", 2),
                description=step_dict.get("description", ""),
                labels=step_dict.get("labels", []) or [],
                depends_on=step_dict.get("depends_on", []) or [],
            )
            steps.append(step)

        workflow = Workflow(
            id=generate_id("wv-workflow"),
            name=name,
            description=description,
            steps=steps,
        )

        return workflow

    def create_or_update_workflow(self, yaml_content: str) -> Workflow:
        """Create or update workflow from YAML.

        If workflow with same name exists, update it (preserve ID).
        """
        workflow = self.parse_workflow_yaml(yaml_content)

        existing = self.storage.find_workflow_by_name(workflow.name)

        if existing:
            workflow.id = existing.id
            workflow.created_at = existing.created_at
            workflow.updated_at = datetime.now()
        else:
            workflow.updated_at = workflow.created_at

        self.storage.write_workflow(workflow)
        return workflow

    def execute_workflow(
        self, workflow_name_or_id: str, label_prefix: str | None = None
    ) -> list[Issue]:
        """Execute a workflow by creating all its issues with dependencies.

        Args:
            workflow_name_or_id: Workflow name or ID
            label_prefix: Optional label prefix (defaults to workflow:{name})

        Returns:
            List of created issues

        Raises:
            ValueError: If workflow not found or dependencies are invalid
        """
        workflow = self.get_workflow(workflow_name_or_id)
        if workflow is None:
            raise ValueError(f"Workflow not found: {workflow_name_or_id}")

        # Validate all depends_on references exist in workflow
        step_titles = {step.title for step in workflow.steps}
        for step in workflow.steps:
            for dep_title in step.depends_on:
                if dep_title not in step_titles:
                    raise ValueError(
                        f"Invalid dependency in step '{step.title}': '{dep_title}' not found in workflow"
                    )

        # Track created issues by step title
        created_issues: dict[str, Issue] = {}

        # Determine workflow label
        workflow_label = f"workflow:{label_prefix or workflow.name}"

        # Create issues for each step
        for step in workflow.steps:
            # Resolve depends_on (step titles) to issue IDs
            blocked_by = [
                created_issues[dep_title].id for dep_title in step.depends_on
            ]

            # Add workflow label
            labels = step.labels.copy()
            labels.append(workflow_label)

            # Create issue
            issue = self.issue_service.create_issue(
                title=step.title,
                type=step.type,
                priority=step.priority,
                description=step.description,
                labels=labels,
                blocked_by=blocked_by,
            )

            created_issues[step.title] = issue

        return list(created_issues.values())

    def get_workflow(self, name_or_id: str) -> Workflow | None:
        """Get workflow by name or ID."""
        # Try by ID first
        workflow = self.storage.read_workflow(name_or_id)
        if workflow is not None:
            return workflow

        # Try by name
        return self.storage.find_workflow_by_name(name_or_id)

    def list_workflows(self) -> list[Workflow]:
        """Return all workflows."""
        return self.storage.list_all_workflows()


class LaunchService:
    """Service for launching AI agents on tasks."""

    def __init__(
        self,
        issue_service: IssueService,
        launch_storage: LaunchStorage,
        hint_service: HintService,
    ):
        self.issue_service = issue_service
        self.launch_storage = launch_storage
        self.hint_service = hint_service

    def prepare_context(self, issue: Issue) -> str:
        """Build markdown context for agent including:
        - Issue details (title, type, priority, description, design notes, acceptance criteria)
        - Relevant hints (based on issue labels)
        - Dependency information (blockers with status)
        """
        parts = []

        # Issue details
        parts.append(f"# Task: {issue.title}\n")
        parts.append(f"**ID**: {issue.id}\n")
        parts.append(f"**Type**: {issue.type.value}\n")
        parts.append(f"**Priority**: P{issue.priority}\n")

        if issue.description:
            parts.append(f"\n## Description\n{issue.description}\n")

        if issue.design_notes:
            parts.append(f"\n## Design Notes\n{issue.design_notes}\n")

        if issue.acceptance_criteria:
            parts.append("\n## Acceptance Criteria\n")
            for criterion in issue.acceptance_criteria:
                parts.append(f"- [ ] {criterion}\n")

        # Add related hints based on labels
        if issue.labels:
            parts.append("\n## Relevant Hints\n")
            for label in issue.labels:
                hint = self.hint_service.get_hint(label)
                if hint:
                    parts.append(f"\n### {hint.title}\n{hint.content}\n")

        # Add dependency information
        graph = self.issue_service._get_graph()
        blockers = graph.blocked_by.get(issue.id, set())
        if blockers:
            parts.append("\n## Dependencies (Blockers)\n")
            for blocker_id in blockers:
                blocker = self.issue_service.get_issue(blocker_id)
                if blocker:
                    parts.append(f"- {blocker.id}: {blocker.title} ({blocker.status.value})\n")

        return "".join(parts)

    def launch_agent(self, issue_id: str, model: AgentModel) -> LaunchExecution:
        """Launch a Claude agent subprocess to work on the issue.

        Raises:
            IssueNotFoundError: If issue_id doesn't exist
        """
        # Get issue
        issue = self.issue_service.get_issue(issue_id)
        if not issue:
            raise IssueNotFoundError(issue_id)

        # Create launch record
        launch = LaunchExecution(
            id=generate_id("wv-launch"),
            issue_id=issue_id,
            model=model,
        )

        # Prepare context
        context = self.prepare_context(issue)

        # Save context to file
        log_file = self.launch_storage.logs_dir / f"{launch.id}.log"
        context_file = self.launch_storage.logs_dir / f"{launch.id}-context.md"
        context_file.write_text(context)
        launch.log_file = str(log_file)

        # Write launch record
        self.launch_storage.write_launch(launch)

        return launch
