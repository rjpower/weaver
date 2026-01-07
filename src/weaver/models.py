"""Data models for Weaver issue tracker."""

import hashlib
import secrets
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum


class Status(Enum):
    OPEN = "open"
    IN_PROGRESS = "in_progress"
    BLOCKED = "blocked"
    CLOSED = "closed"


class IssueType(Enum):
    TASK = "task"
    BUG = "bug"
    FEATURE = "feature"
    EPIC = "epic"
    CHORE = "chore"


@dataclass
class Issue:
    id: str
    title: str
    status: Status = Status.OPEN
    type: IssueType = IssueType.TASK
    priority: int = 2  # 0-4, lower = higher priority
    description: str = ""
    design_notes: str = ""
    acceptance_criteria: list[str] = field(default_factory=list)
    labels: list[str] = field(default_factory=list)
    blocked_by: list[str] = field(default_factory=list)
    parent: str | None = None
    created_at: datetime = field(default_factory=datetime.now)
    updated_at: datetime = field(default_factory=datetime.now)
    closed_at: datetime | None = None

    @property
    def content_hash(self) -> str:
        """SHA256 of title + description + design for dedup detection."""
        content = f"{self.title}|{self.description}|{self.design_notes}"
        return hashlib.sha256(content.encode()).hexdigest()[:12]

    def is_open(self) -> bool:
        """Check if issue is in an open state (not closed)."""
        return self.status in (Status.OPEN, Status.IN_PROGRESS, Status.BLOCKED)


def generate_id(prefix: str = "wv") -> str:
    """Generate a short hash-based ID like 'wv-a3f8'."""
    return f"{prefix}-{secrets.token_hex(2)}"
