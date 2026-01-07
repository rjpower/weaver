"""Tests for utility functions."""

import pytest

from weaver.utils import truncate_content


def test_truncate_content_short_text():
    """Text shorter than max_words is not truncated."""
    text = "This is a short text"
    result, was_truncated = truncate_content(text, max_words=10)

    assert result == text
    assert was_truncated is False


def test_truncate_content_exact_length():
    """Text with exactly max_words is not truncated."""
    text = "one two three four five"
    result, was_truncated = truncate_content(text, max_words=5)

    assert result == text
    assert was_truncated is False


def test_truncate_content_long_text():
    """Text longer than max_words is truncated with ellipsis."""
    text = "one two three four five six seven eight"
    result, was_truncated = truncate_content(text, max_words=5)

    assert result == "one two three four five..."
    assert was_truncated is True


def test_truncate_content_default_max_words():
    """Default max_words is 200."""
    # Create text with 250 words
    words = [f"word{i}" for i in range(250)]
    text = " ".join(words)

    result, was_truncated = truncate_content(text)

    # Should truncate to 200 words + ellipsis
    expected_words = words[:200]
    expected = " ".join(expected_words) + "..."

    assert result == expected
    assert was_truncated is True


def test_truncate_content_preserves_word_boundaries():
    """Truncation respects word boundaries."""
    text = "alpha bravo charlie delta echo foxtrot"
    result, was_truncated = truncate_content(text, max_words=3)

    assert result == "alpha bravo charlie..."
    assert was_truncated is True
    # Verify no partial words
    assert "delta" not in result


def test_truncate_content_empty_string():
    """Empty string returns empty without truncation."""
    result, was_truncated = truncate_content("", max_words=10)

    assert result == ""
    assert was_truncated is False


def test_truncate_content_single_word():
    """Single word text is handled correctly."""
    text = "word"
    result, was_truncated = truncate_content(text, max_words=1)

    assert result == text
    assert was_truncated is False


def test_truncate_content_whitespace_handling():
    """Multiple whitespace characters are handled by split."""
    text = "one  two   three    four"
    result, was_truncated = truncate_content(text, max_words=2)

    assert result == "one two..."
    assert was_truncated is True


@pytest.mark.parametrize(
    "text,max_words,expected_truncated",
    [
        ("a b c", 5, False),
        ("a b c", 3, False),
        ("a b c", 2, True),
        ("a b c d e f", 3, True),
        ("", 10, False),
    ],
)
def test_truncate_content_parametrized(text, max_words, expected_truncated):
    """Parametrized test for various truncation scenarios."""
    _, was_truncated = truncate_content(text, max_words)
    assert was_truncated == expected_truncated
