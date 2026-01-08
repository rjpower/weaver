"""Markdown file storage for Weaver issue tracker."""

import re
from datetime import datetime
from pathlib import Path

import frontmatter
import yaml

from weaver.models import (
    AgentModel,
    Hint,
    Issue,
    LaunchExecution,
    Status,
    Workflow,
    WorkflowStep,
)


class MarkdownStorage:
    """Read/write issues as markdown files with YAML frontmatter."""

    def __init__(self, root: Path):
        self.root = root
        self.issues_dir = root / "issues"
        self.index_path = root / "index.yml"

    def ensure_initialized(self) -> None:
        """Create .weaver/issues/ directory if not exists."""
        self.issues_dir.mkdir(parents=True, exist_ok=True)
        if not self.index_path.exists():
            self._save_index({"issues": {}})

    def issue_path(self, issue_id: str) -> Path:
        """Get the path to an issue's markdown file."""
        return self.issues_dir / f"{issue_id}.md"

    def read_issue(self, issue_id: str) -> Issue | None:
        """Read and parse a single issue file."""
        path = self.issue_path(issue_id)
        if not path.exists():
            return None
        post = frontmatter.load(path)
        return self._parse_issue(post)

    def write_issue(self, issue: Issue) -> None:
        """Write issue to markdown file and update index."""
        path = self.issue_path(issue.id)
        content = self._serialize_issue(issue)
        path.write_text(content)
        self._update_index(issue)

    def delete_issue(self, issue_id: str) -> bool:
        """Delete issue file and remove from index. Returns True if deleted."""
        path = self.issue_path(issue_id)
        if path.exists():
            path.unlink()
            self._remove_from_index(issue_id)
            return True
        return False

    def list_issue_ids(self) -> list[str]:
        """List all issue IDs from filenames."""
        if not self.issues_dir.exists():
            return []
        return [p.stem for p in self.issues_dir.glob("*.md")]

    def read_all_issues(self) -> list[Issue]:
        """Read all issues from storage."""
        return [
            issue
            for issue_id in self.list_issue_ids()
            if (issue := self.read_issue(issue_id)) is not None
        ]

    def _load_index(self) -> dict:
        """Load index from index.yml."""
        if not self.index_path.exists():
            return {"issues": {}}
        with open(self.index_path) as f:
            return yaml.safe_load(f) or {"issues": {}}

    def _save_index(self, index: dict) -> None:
        """Save index to index.yml."""
        with open(self.index_path, "w") as f:
            yaml.dump(index, f, default_flow_style=False, sort_keys=False)

    def _update_index(self, issue: Issue) -> None:
        """Update index entry for an issue."""
        index = self._load_index()
        index["issues"][issue.id] = {
            "title": issue.title,
            "status": issue.status.value,
            "priority": issue.priority,
            "labels": issue.labels,
            "blocked_by": issue.blocked_by,
            "updated_at": issue.updated_at.isoformat(),
        }
        self._save_index(index)

    def _remove_from_index(self, issue_id: str) -> None:
        """Remove issue from index."""
        index = self._load_index()
        if issue_id in index.get("issues", {}):
            del index["issues"][issue_id]
            self._save_index(index)

    def _parse_issue(self, post: frontmatter.Post) -> Issue:
        """Parse frontmatter Post into Issue dataclass."""
        metadata = post.metadata
        content = post.content

        # Parse description (text before any ## heading)
        description = ""
        design_notes = ""
        acceptance_criteria: list[str] = []

        lines = content.split("\n")
        current_section = "description"
        section_lines: dict[str, list[str]] = {
            "description": [],
            "design_notes": [],
            "acceptance_criteria": [],
        }

        for line in lines:
            if line.startswith("## Design Notes"):
                current_section = "design_notes"
                continue
            elif line.startswith("## Acceptance Criteria"):
                current_section = "acceptance_criteria"
                continue
            elif line.startswith("## "):
                current_section = "other"
                continue

            if current_section in section_lines:
                section_lines[current_section].append(line)

        description = "\n".join(section_lines["description"]).strip()
        design_notes = "\n".join(section_lines["design_notes"]).strip()

        # Parse acceptance criteria checkboxes
        for line in section_lines["acceptance_criteria"]:
            match = re.match(r"^\s*-\s*\[[ x]\]\s*(.+)$", line.strip())
            if match:
                acceptance_criteria.append(match.group(1))

        # Parse datetime fields
        created_at = self._parse_datetime(metadata.get("created_at"))
        updated_at = self._parse_datetime(metadata.get("updated_at"))
        closed_at = self._parse_datetime(metadata.get("closed_at"))

        return Issue(
            id=metadata["id"],
            title=metadata["title"],
            status=Status(metadata.get("status", "open")),
            priority=metadata.get("priority", 2),
            description=description,
            design_notes=design_notes,
            acceptance_criteria=acceptance_criteria,
            labels=metadata.get("labels", []) or [],
            blocked_by=metadata.get("blocked_by", []) or [],
            parent=metadata.get("parent"),
            created_at=created_at,
            updated_at=updated_at,
            closed_at=closed_at,
        )

    def _parse_datetime(self, value: str | datetime | None) -> datetime:
        """Parse datetime from string or return datetime directly."""
        if value is None:
            return datetime.now()
        if isinstance(value, datetime):
            return value
        return datetime.fromisoformat(value)

    def _serialize_issue(self, issue: Issue) -> str:
        """Serialize Issue to markdown with YAML frontmatter."""
        metadata = {
            "id": issue.id,
            "title": issue.title,
            "status": issue.status.value,
            "priority": issue.priority,
            "labels": issue.labels,
            "blocked_by": issue.blocked_by,
            "parent": issue.parent,
            "created_at": issue.created_at.isoformat(),
            "updated_at": issue.updated_at.isoformat(),
        }
        if issue.closed_at:
            metadata["closed_at"] = issue.closed_at.isoformat()

        # Build content body
        content_parts = []

        if issue.description:
            content_parts.append(issue.description)

        if issue.design_notes:
            content_parts.append(f"## Design Notes\n\n{issue.design_notes}")

        if issue.acceptance_criteria:
            criteria_lines = [
                f"- [ ] {criterion}" for criterion in issue.acceptance_criteria
            ]
            content_parts.append(
                f"## Acceptance Criteria\n\n" + "\n".join(criteria_lines)
            )

        content = "\n\n".join(content_parts)

        post = frontmatter.Post(content, **metadata)
        return frontmatter.dumps(post)


class HintStorage:
    """Read/write hints as markdown files with YAML frontmatter."""

    def __init__(self, root: Path):
        self.root = root
        self.hints_dir = root / "hints"
        self.index_path = root / "hints_index.yml"

    def ensure_initialized(self) -> None:
        """Create .weaver/hints/ directory and index if not exists."""
        self.hints_dir.mkdir(parents=True, exist_ok=True)
        if not self.index_path.exists():
            self._save_index({"hints": {}})

    def hint_path(self, hint_id: str) -> Path:
        """Get path to a hint's markdown file."""
        return self.hints_dir / f"{hint_id}.md"

    def read_hint(self, hint_id: str) -> Hint | None:
        """Read and parse a single hint file."""
        path = self.hint_path(hint_id)
        if not path.exists():
            return None
        post = frontmatter.load(path)
        return self._parse_hint(post)

    def write_hint(self, hint: Hint) -> None:
        """Write hint to markdown file and update index."""
        path = self.hint_path(hint.id)
        content = self._serialize_hint(hint)
        path.write_text(content)
        self._update_index(hint)

    def find_hint_by_title(self, title: str) -> Hint | None:
        """Find hint by title (case-insensitive)."""
        title_lower = title.lower()
        for hint in self.list_all_hints():
            if hint.title.lower() == title_lower:
                return hint
        return None

    def list_all_hints(self) -> list[Hint]:
        """Return all hints sorted by title."""
        hint_ids = self._list_hint_ids()
        hints = [
            hint
            for hint_id in hint_ids
            if (hint := self.read_hint(hint_id)) is not None
        ]
        return sorted(hints, key=lambda h: h.title.lower())

    def search_hints(self, query: str) -> list[Hint]:
        """Search in title and content (case-insensitive)."""
        query_lower = query.lower()
        results = []
        for hint in self.list_all_hints():
            if (
                query_lower in hint.title.lower()
                or query_lower in hint.content.lower()
            ):
                results.append(hint)
        return results

    def _list_hint_ids(self) -> list[str]:
        """List all hint IDs from filenames."""
        if not self.hints_dir.exists():
            return []
        return [p.stem for p in self.hints_dir.glob("*.md")]

    def _load_index(self) -> dict:
        """Load index from hints_index.yml."""
        if not self.index_path.exists():
            return {"hints": {}}
        with open(self.index_path) as f:
            return yaml.safe_load(f) or {"hints": {}}

    def _save_index(self, index: dict) -> None:
        """Save index to hints_index.yml."""
        with open(self.index_path, "w") as f:
            yaml.dump(index, f, default_flow_style=False, sort_keys=False)

    def _update_index(self, hint: Hint) -> None:
        """Update index entry for a hint."""
        index = self._load_index()
        index["hints"][hint.id] = {
            "title": hint.title,
            "labels": hint.labels,
            "updated_at": hint.updated_at.isoformat(),
        }
        self._save_index(index)

    def _parse_hint(self, post: frontmatter.Post) -> Hint:
        """Parse frontmatter Post into Hint dataclass."""
        metadata = post.metadata
        content = post.content

        created_at = self._parse_datetime(metadata.get("created_at"))
        updated_at = self._parse_datetime(metadata.get("updated_at"))

        return Hint(
            id=metadata["id"],
            title=metadata["title"],
            content=content,
            labels=metadata.get("labels", []) or [],
            created_at=created_at,
            updated_at=updated_at,
        )

    def _parse_datetime(self, value: str | datetime | None) -> datetime:
        """Parse datetime from string or return datetime directly."""
        if value is None:
            return datetime.now()
        if isinstance(value, datetime):
            return value
        return datetime.fromisoformat(value)

    def _serialize_hint(self, hint: Hint) -> str:
        """Serialize Hint to markdown with YAML frontmatter."""
        metadata = {
            "id": hint.id,
            "title": hint.title,
            "labels": hint.labels,
            "created_at": hint.created_at.isoformat(),
            "updated_at": hint.updated_at.isoformat(),
        }

        post = frontmatter.Post(hint.content, **metadata)
        return frontmatter.dumps(post)


class WorkflowStorage:
    """Read/write workflows as YAML files."""

    def __init__(self, root: Path):
        self.root = root
        self.workflows_dir = root / "workflows"

    def ensure_initialized(self) -> None:
        """Create .weaver/workflows/ directory if not exists."""
        self.workflows_dir.mkdir(parents=True, exist_ok=True)

    def workflow_path(self, workflow_id: str) -> Path:
        """Get path to a workflow's YAML file."""
        return self.workflows_dir / f"{workflow_id}.yml"

    def write_workflow(self, workflow: Workflow) -> None:
        """Save workflow as YAML."""
        data = self._serialize_workflow(workflow)
        path = self.workflow_path(workflow.id)
        with open(path, "w") as f:
            yaml.dump(data, f, default_flow_style=False, sort_keys=False)

    def read_workflow(self, workflow_id: str) -> Workflow | None:
        """Read and parse a workflow file."""
        path = self.workflow_path(workflow_id)
        if not path.exists():
            return None
        with open(path) as f:
            data = yaml.safe_load(f)
        return self._deserialize_workflow(data)

    def find_workflow_by_name(self, name: str) -> Workflow | None:
        """Find workflow by name (case-insensitive)."""
        name_lower = name.lower()
        for workflow in self.list_all_workflows():
            if workflow.name.lower() == name_lower:
                return workflow
        return None

    def list_all_workflows(self) -> list[Workflow]:
        """Return all workflows sorted by name."""
        if not self.workflows_dir.exists():
            return []
        workflows = []
        for path in self.workflows_dir.glob("*.yml"):
            workflow_id = path.stem
            workflow = self.read_workflow(workflow_id)
            if workflow:
                workflows.append(workflow)
        return sorted(workflows, key=lambda w: w.name.lower())

    def _serialize_workflow(self, workflow: Workflow) -> dict:
        """Serialize Workflow to dict for YAML."""
        return {
            "id": workflow.id,
            "name": workflow.name,
            "description": workflow.description,
            "created_at": workflow.created_at.isoformat(),
            "updated_at": workflow.updated_at.isoformat(),
            "steps": [self._serialize_step(step) for step in workflow.steps],
        }

    def _serialize_step(self, step: WorkflowStep) -> dict:
        """Serialize WorkflowStep to dict for YAML."""
        return {
            "title": step.title,
            "priority": step.priority,
            "description": step.description,
            "labels": step.labels,
            "depends_on": step.depends_on,
        }

    def _deserialize_workflow(self, data: dict) -> Workflow:
        """Deserialize dict to Workflow object."""
        return Workflow(
            id=data["id"],
            name=data["name"],
            description=data.get("description", ""),
            created_at=datetime.fromisoformat(data["created_at"]),
            updated_at=datetime.fromisoformat(data["updated_at"]),
            steps=[self._deserialize_step(step_data) for step_data in data.get("steps", [])],
        )

    def _deserialize_step(self, data: dict) -> WorkflowStep:
        """Deserialize dict to WorkflowStep object."""
        return WorkflowStep(
            title=data["title"],
            priority=data.get("priority", 2),
            description=data.get("description", ""),
            labels=data.get("labels", []) or [],
            depends_on=data.get("depends_on", []) or [],
        )


class LaunchStorage:
    """Read/write launch executions as YAML files."""

    def __init__(self, root: Path):
        self.root = root
        self.launches_dir = root / "launches"
        self.logs_dir = root / "launches" / "logs"

    def ensure_initialized(self) -> None:
        """Create .weaver/launches/ and logs/ directories if not exist."""
        self.launches_dir.mkdir(parents=True, exist_ok=True)
        self.logs_dir.mkdir(parents=True, exist_ok=True)

    def launch_path(self, launch_id: str) -> Path:
        """Get path to a launch's YAML file."""
        return self.launches_dir / f"{launch_id}.yml"

    def write_launch(self, launch: LaunchExecution) -> None:
        """Save launch execution metadata as YAML."""
        data = {
            "id": launch.id,
            "issue_id": launch.issue_id,
            "model": launch.model.value,
            "started_at": launch.started_at.isoformat(),
            "completed_at": launch.completed_at.isoformat() if launch.completed_at else None,
            "exit_code": launch.exit_code,
            "log_file": launch.log_file,
        }

        path = self.launch_path(launch.id)
        with open(path, "w") as f:
            yaml.dump(data, f, default_flow_style=False, sort_keys=False)

    def read_launch(self, launch_id: str) -> LaunchExecution | None:
        """Read and parse a launch execution file."""
        path = self.launch_path(launch_id)
        if not path.exists():
            return None

        with open(path) as f:
            data = yaml.safe_load(f)

        if not data:
            return None

        return LaunchExecution(
            id=data["id"],
            issue_id=data["issue_id"],
            model=AgentModel(data["model"]),
            started_at=datetime.fromisoformat(data["started_at"]),
            completed_at=datetime.fromisoformat(data["completed_at"]) if data.get("completed_at") else None,
            exit_code=data.get("exit_code"),
            log_file=data.get("log_file", ""),
        )

    def list_launches_for_issue(self, issue_id: str) -> list[LaunchExecution]:
        """Find all launch executions for an issue."""
        if not self.launches_dir.exists():
            return []

        launches = []
        for path in self.launches_dir.glob("*.yml"):
            launch = self.read_launch(path.stem)
            if launch and launch.issue_id == issue_id:
                launches.append(launch)

        return launches
