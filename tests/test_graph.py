"""Tests for weaver.graph."""

import pytest

from weaver.graph import DependencyGraph
from weaver.models import Issue, Status


def make_issue(id: str, blocked_by: list[str] | None = None, status: Status = Status.OPEN) -> Issue:
    """Helper to create test issues."""
    return Issue(
        id=id,
        title=f"Issue {id}",
        status=status,
        blocked_by=blocked_by or [],
    )


class TestDependencyGraphBuild:
    def test_build_empty(self):
        graph = DependencyGraph.build([])
        assert graph.all_ids == set()
        assert graph.blocked_by == {}
        assert graph.blocks == {}

    def test_build_no_dependencies(self):
        issues = [make_issue("wv-1"), make_issue("wv-2")]
        graph = DependencyGraph.build(issues)

        assert graph.all_ids == {"wv-1", "wv-2"}
        assert graph.blocked_by == {}
        assert graph.blocks == {}

    def test_build_with_dependencies(self):
        # wv-2 is blocked by wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        assert graph.blocked_by == {"wv-2": {"wv-1"}}
        assert graph.blocks == {"wv-1": {"wv-2"}}

    def test_build_multiple_dependencies(self):
        # wv-3 blocked by both wv-1 and wv-2
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2"),
            make_issue("wv-3", blocked_by=["wv-1", "wv-2"]),
        ]
        graph = DependencyGraph.build(issues)

        assert graph.blocked_by["wv-3"] == {"wv-1", "wv-2"}
        assert graph.blocks["wv-1"] == {"wv-3"}
        assert graph.blocks["wv-2"] == {"wv-3"}


class TestIsBlocked:
    def test_not_blocked_when_no_deps(self):
        issues = [make_issue("wv-1")]
        graph = DependencyGraph.build(issues)

        assert graph.is_blocked("wv-1", {"wv-1"}) is False

    def test_blocked_when_blocker_open(self):
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)
        open_ids = {"wv-1", "wv-2"}

        assert graph.is_blocked("wv-2", open_ids) is True

    def test_not_blocked_when_blocker_closed(self):
        issues = [
            make_issue("wv-1", status=Status.CLOSED),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)
        open_ids = {"wv-2"}  # wv-1 is closed, not in open_ids

        assert graph.is_blocked("wv-2", open_ids) is False

    def test_blocked_by_any_open_blocker(self):
        # wv-3 blocked by wv-1 (closed) and wv-2 (open)
        issues = [
            make_issue("wv-1", status=Status.CLOSED),
            make_issue("wv-2"),
            make_issue("wv-3", blocked_by=["wv-1", "wv-2"]),
        ]
        graph = DependencyGraph.build(issues)
        open_ids = {"wv-2", "wv-3"}

        assert graph.is_blocked("wv-3", open_ids) is True


class TestGetUnblocked:
    def test_all_unblocked_when_no_deps(self):
        issues = [make_issue("wv-1"), make_issue("wv-2")]
        graph = DependencyGraph.build(issues)

        unblocked = graph.get_unblocked(issues)
        ids = {i.id for i in unblocked}
        assert ids == {"wv-1", "wv-2"}

    def test_excludes_blocked_issues(self):
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        unblocked = graph.get_unblocked(issues)
        ids = {i.id for i in unblocked}
        assert ids == {"wv-1"}

    def test_excludes_manually_blocked_status(self):
        issues = [
            make_issue("wv-1", status=Status.BLOCKED),
            make_issue("wv-2"),
        ]
        graph = DependencyGraph.build(issues)

        unblocked = graph.get_unblocked(issues)
        ids = {i.id for i in unblocked}
        assert ids == {"wv-2"}

    def test_excludes_closed_issues(self):
        issues = [
            make_issue("wv-1", status=Status.CLOSED),
            make_issue("wv-2"),
        ]
        graph = DependencyGraph.build(issues)

        unblocked = graph.get_unblocked(issues)
        ids = {i.id for i in unblocked}
        assert ids == {"wv-2"}

    def test_includes_issue_when_blocker_closed(self):
        issues = [
            make_issue("wv-1", status=Status.CLOSED),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        unblocked = graph.get_unblocked(issues)
        ids = {i.id for i in unblocked}
        assert ids == {"wv-2"}


class TestDetectCycle:
    def test_no_cycle_in_valid_dep(self):
        issues = [make_issue("wv-1"), make_issue("wv-2")]
        graph = DependencyGraph.build(issues)

        # Adding wv-2 blocked by wv-1 should be fine
        assert graph.detect_cycle("wv-2", "wv-1") is False

    def test_direct_cycle(self):
        # wv-2 is already blocked by wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        # Adding wv-1 blocked by wv-2 would create: wv-1 -> wv-2 -> wv-1
        assert graph.detect_cycle("wv-1", "wv-2") is True

    def test_transitive_cycle(self):
        # Chain: wv-3 -> wv-2 -> wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
            make_issue("wv-3", blocked_by=["wv-2"]),
        ]
        graph = DependencyGraph.build(issues)

        # Adding wv-1 blocked by wv-3 would create cycle
        assert graph.detect_cycle("wv-1", "wv-3") is True

    def test_self_dependency_is_cycle(self):
        issues = [make_issue("wv-1")]
        graph = DependencyGraph.build(issues)

        assert graph.detect_cycle("wv-1", "wv-1") is True

    def test_no_cycle_in_diamond(self):
        # Diamond: wv-4 depends on wv-2 and wv-3, both depend on wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
            make_issue("wv-3", blocked_by=["wv-1"]),
            make_issue("wv-4", blocked_by=["wv-2", "wv-3"]),
        ]
        graph = DependencyGraph.build(issues)

        # Adding another dependency from wv-4 to wv-1 is redundant but not a cycle
        assert graph.detect_cycle("wv-4", "wv-1") is False


class TestGetBlockers:
    def test_empty_when_no_blockers(self):
        issues = [make_issue("wv-1")]
        graph = DependencyGraph.build(issues)

        assert graph.get_blockers("wv-1") == set()

    def test_returns_blockers(self):
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2"),
            make_issue("wv-3", blocked_by=["wv-1", "wv-2"]),
        ]
        graph = DependencyGraph.build(issues)

        assert graph.get_blockers("wv-3") == {"wv-1", "wv-2"}

    def test_returns_copy(self):
        issues = [make_issue("wv-2", blocked_by=["wv-1"])]
        graph = DependencyGraph.build(issues)

        blockers = graph.get_blockers("wv-2")
        blockers.add("wv-999")
        assert "wv-999" not in graph.blocked_by.get("wv-2", set())


class TestGetBlockedByThis:
    def test_empty_when_not_blocking(self):
        issues = [make_issue("wv-1")]
        graph = DependencyGraph.build(issues)

        assert graph.get_blocked_by_this("wv-1") == set()

    def test_returns_blocked_issues(self):
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
            make_issue("wv-3", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        assert graph.get_blocked_by_this("wv-1") == {"wv-2", "wv-3"}


class TestGetTransitiveBlockers:
    def test_empty_when_no_blockers(self):
        issues = [make_issue("wv-1")]
        graph = DependencyGraph.build(issues)

        assert graph.get_transitive_blockers("wv-1") == []

    def test_single_blocker(self):
        # wv-2 blocked by wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        assert graph.get_transitive_blockers("wv-2") == ["wv-1"]

    def test_chain_of_blockers(self):
        # Chain: wv-3 -> wv-2 -> wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
            make_issue("wv-3", blocked_by=["wv-2"]),
        ]
        graph = DependencyGraph.build(issues)

        # Should return deepest first: wv-1, then wv-2
        result = graph.get_transitive_blockers("wv-3")
        assert result == ["wv-1", "wv-2"]

    def test_multiple_direct_blockers(self):
        # wv-3 blocked by both wv-1 and wv-2
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2"),
            make_issue("wv-3", blocked_by=["wv-1", "wv-2"]),
        ]
        graph = DependencyGraph.build(issues)

        result = graph.get_transitive_blockers("wv-3")
        # Both should be included, order depends on iteration
        assert set(result) == {"wv-1", "wv-2"}
        assert len(result) == 2

    def test_diamond_dependency(self):
        # Diamond: wv-4 depends on wv-2 and wv-3, both depend on wv-1
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
            make_issue("wv-3", blocked_by=["wv-1"]),
            make_issue("wv-4", blocked_by=["wv-2", "wv-3"]),
        ]
        graph = DependencyGraph.build(issues)

        result = graph.get_transitive_blockers("wv-4")
        # wv-1 should appear before wv-2 and wv-3 (deepest first)
        assert result[0] == "wv-1"
        assert set(result[1:]) == {"wv-2", "wv-3"}
        assert len(result) == 3

    def test_complex_dag(self):
        # Complex: wv-5 -> [wv-3, wv-4]
        #          wv-3 -> [wv-1, wv-2]
        #          wv-4 -> wv-2
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2"),
            make_issue("wv-3", blocked_by=["wv-1", "wv-2"]),
            make_issue("wv-4", blocked_by=["wv-2"]),
            make_issue("wv-5", blocked_by=["wv-3", "wv-4"]),
        ]
        graph = DependencyGraph.build(issues)

        result = graph.get_transitive_blockers("wv-5")
        # All should be included
        assert set(result) == {"wv-1", "wv-2", "wv-3", "wv-4"}
        # wv-1 and wv-2 should come before wv-3
        # wv-2 should come before wv-4
        # wv-3 and wv-4 should come before wv-5 (but wv-5 is excluded)
        wv1_idx = result.index("wv-1")
        wv2_idx = result.index("wv-2")
        wv3_idx = result.index("wv-3")
        wv4_idx = result.index("wv-4")
        assert wv1_idx < wv3_idx
        assert wv2_idx < wv3_idx
        assert wv2_idx < wv4_idx

    def test_does_not_include_starting_issue(self):
        # Verify the starting issue is never in the result
        issues = [
            make_issue("wv-1"),
            make_issue("wv-2", blocked_by=["wv-1"]),
        ]
        graph = DependencyGraph.build(issues)

        result = graph.get_transitive_blockers("wv-2")
        assert "wv-2" not in result
        assert result == ["wv-1"]

    def test_handles_cycle_gracefully(self):
        # Manually construct a graph with a cycle (shouldn't happen in practice)
        graph = DependencyGraph(
            blocked_by={"wv-1": {"wv-2"}, "wv-2": {"wv-1"}},
            blocks={"wv-1": {"wv-2"}, "wv-2": {"wv-1"}},
            all_ids={"wv-1", "wv-2"},
        )

        # Should handle cycle via visited set
        result = graph.get_transitive_blockers("wv-1")
        # Should visit wv-2 once and not infinitely loop
        assert "wv-2" in result
        assert "wv-1" not in result
        assert len(result) == 1
