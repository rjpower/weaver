"""Tests for HintStorage."""

from datetime import datetime
from pathlib import Path

import pytest
import yaml

from weaver.models import Hint
from weaver.storage import HintStorage


@pytest.fixture
def weaver_root(tmp_path: Path) -> Path:
    root = tmp_path / ".weaver"
    root.mkdir()
    return root


@pytest.fixture
def storage(weaver_root: Path) -> HintStorage:
    storage = HintStorage(weaver_root)
    storage.ensure_initialized()
    return storage


class TestHintStorage:
    def test_ensure_initialized_creates_directories(self, tmp_path: Path):
        root = tmp_path / ".weaver"
        storage = HintStorage(root)
        storage.ensure_initialized()

        assert (root / "hints").is_dir()
        assert (root / "hints_index.yml").exists()

    def test_hint_path(self, storage: HintStorage, weaver_root: Path):
        path = storage.hint_path("wv-hint-1234")
        assert path == weaver_root / "hints" / "wv-hint-1234.md"

    def test_write_and_read_hint_roundtrip(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-test",
            title="cli",
            content="The CLI code lives in src/weaver/cli.py...",
            labels=["documentation", "cli"],
            created_at=datetime(2026, 1, 7, 10, 0, 0),
            updated_at=datetime(2026, 1, 7, 11, 0, 0),
        )

        storage.write_hint(hint)
        loaded = storage.read_hint("wv-hint-test")

        assert loaded is not None
        assert loaded.id == hint.id
        assert loaded.title == hint.title
        assert loaded.content == hint.content
        assert loaded.labels == hint.labels
        assert loaded.created_at == hint.created_at
        assert loaded.updated_at == hint.updated_at

    def test_read_nonexistent_hint_returns_none(self, storage: HintStorage):
        assert storage.read_hint("wv-hint-nonexistent") is None

    def test_write_hint_with_multiline_content(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-multi",
            title="multiline",
            content="Line 1\n\nLine 2\n\nLine 3",
            labels=["test"],
        )
        storage.write_hint(hint)
        loaded = storage.read_hint("wv-hint-multi")

        assert loaded is not None
        assert loaded.content == "Line 1\n\nLine 2\n\nLine 3"

    def test_write_hint_with_empty_labels(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-empty",
            title="no labels",
            content="Some content",
            labels=[],
        )
        storage.write_hint(hint)
        loaded = storage.read_hint("wv-hint-empty")

        assert loaded is not None
        assert loaded.labels == []

    def test_find_hint_by_title_exact_case(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-1",
            title="CLI Commands",
            content="Command documentation",
        )
        storage.write_hint(hint)

        found = storage.find_hint_by_title("CLI Commands")
        assert found is not None
        assert found.id == "wv-hint-1"

    def test_find_hint_by_title_case_insensitive(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-2",
            title="Storage Layer",
            content="Storage documentation",
        )
        storage.write_hint(hint)

        found = storage.find_hint_by_title("storage layer")
        assert found is not None
        assert found.id == "wv-hint-2"

    def test_find_hint_by_title_not_found(self, storage: HintStorage):
        hint = Hint(
            id="wv-hint-3",
            title="Testing",
            content="Test documentation",
        )
        storage.write_hint(hint)

        found = storage.find_hint_by_title("nonexistent")
        assert found is None

    def test_list_all_hints_empty(self, storage: HintStorage):
        hints = storage.list_all_hints()
        assert hints == []

    def test_list_all_hints_sorted_by_title(self, storage: HintStorage):
        storage.write_hint(
            Hint(id="wv-hint-z", title="Zebra", content="Last alphabetically")
        )
        storage.write_hint(
            Hint(id="wv-hint-a", title="Apple", content="First alphabetically")
        )
        storage.write_hint(
            Hint(id="wv-hint-m", title="Mango", content="Middle alphabetically")
        )

        hints = storage.list_all_hints()
        assert len(hints) == 3
        assert hints[0].title == "Apple"
        assert hints[1].title == "Mango"
        assert hints[2].title == "Zebra"

    def test_list_all_hints_case_insensitive_sort(self, storage: HintStorage):
        storage.write_hint(Hint(id="wv-hint-1", title="banana", content="b"))
        storage.write_hint(Hint(id="wv-hint-2", title="Apple", content="a"))
        storage.write_hint(Hint(id="wv-hint-3", title="Cherry", content="c"))

        hints = storage.list_all_hints()
        assert hints[0].title == "Apple"
        assert hints[1].title == "banana"
        assert hints[2].title == "Cherry"

    def test_search_hints_by_title(self, storage: HintStorage):
        storage.write_hint(Hint(id="wv-hint-1", title="CLI Commands", content="cli"))
        storage.write_hint(
            Hint(id="wv-hint-2", title="Storage Layer", content="storage")
        )
        storage.write_hint(Hint(id="wv-hint-3", title="CLI Tools", content="tools"))

        results = storage.search_hints("CLI")
        assert len(results) == 2
        titles = {r.title for r in results}
        assert titles == {"CLI Commands", "CLI Tools"}

    def test_search_hints_by_content(self, storage: HintStorage):
        storage.write_hint(
            Hint(id="wv-hint-1", title="First", content="This mentions pytest")
        )
        storage.write_hint(
            Hint(id="wv-hint-2", title="Second", content="This uses unittest")
        )
        storage.write_hint(
            Hint(id="wv-hint-3", title="Third", content="Testing with pytest")
        )

        results = storage.search_hints("pytest")
        assert len(results) == 2
        titles = {r.title for r in results}
        assert titles == {"First", "Third"}

    def test_search_hints_case_insensitive(self, storage: HintStorage):
        storage.write_hint(
            Hint(id="wv-hint-1", title="Python Guide", content="Python basics")
        )

        results = storage.search_hints("python")
        assert len(results) == 1
        assert results[0].title == "Python Guide"

        results = storage.search_hints("PYTHON")
        assert len(results) == 1
        assert results[0].title == "Python Guide"

    def test_search_hints_no_results(self, storage: HintStorage):
        storage.write_hint(Hint(id="wv-hint-1", title="Test", content="Content"))

        results = storage.search_hints("nonexistent")
        assert results == []

    def test_search_hints_matches_both_title_and_content(self, storage: HintStorage):
        storage.write_hint(
            Hint(id="wv-hint-1", title="Search Test", content="other content")
        )
        storage.write_hint(
            Hint(id="wv-hint-2", title="Other Title", content="search in content")
        )

        results = storage.search_hints("search")
        assert len(results) == 2


class TestHintIndex:
    def test_write_updates_index(self, storage: HintStorage, weaver_root: Path):
        hint = Hint(
            id="wv-hint-idx",
            title="Indexed Hint",
            content="Some content",
            labels=["test", "index"],
        )
        storage.write_hint(hint)

        with open(weaver_root / "hints_index.yml") as f:
            index = yaml.safe_load(f)

        assert "wv-hint-idx" in index["hints"]
        entry = index["hints"]["wv-hint-idx"]
        assert entry["title"] == "Indexed Hint"
        assert entry["labels"] == ["test", "index"]
        assert "updated_at" in entry


class TestHintMarkdownFormat:
    def test_file_has_yaml_frontmatter(self, storage: HintStorage, weaver_root: Path):
        hint = Hint(id="wv-hint-fmt", title="Format Test", content="Content here")
        storage.write_hint(hint)

        content = (weaver_root / "hints" / "wv-hint-fmt.md").read_text()
        assert content.startswith("---\n")
        assert "\n---" in content

    def test_frontmatter_contains_required_fields(
        self, storage: HintStorage, weaver_root: Path
    ):
        hint = Hint(
            id="wv-hint-fields",
            title="Field Test",
            content="Content",
            labels=["label1"],
            created_at=datetime(2026, 1, 7, 10, 0, 0),
            updated_at=datetime(2026, 1, 7, 11, 0, 0),
        )
        storage.write_hint(hint)

        loaded = storage.read_hint("wv-hint-fields")
        assert loaded is not None
        assert loaded.id == "wv-hint-fields"
        assert loaded.title == "Field Test"
        assert loaded.labels == ["label1"]
        assert loaded.created_at == datetime(2026, 1, 7, 10, 0, 0)
        assert loaded.updated_at == datetime(2026, 1, 7, 11, 0, 0)
