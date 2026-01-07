"""Markdown file storage for Weaver issue tracker."""

import re
from datetime import datetime
from pathlib import Path

import frontmatter
import yaml

from weaver.models import Issue, IssueType, Status


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
            "type": issue.type.value,
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
            type=IssueType(metadata.get("type", "task")),
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
            "type": issue.type.value,
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
