"""Tests for utility functions."""

import os
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

from weaver.utils import truncate_content, extract_conversation_from_log, summarize_conversation_log


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


def test_extract_conversation_from_log(tmp_path):
    """Test extracting conversation from a log file."""
    from claude_agent_sdk import AssistantMessage, UserMessage, TextBlock
    from weaver.utils import serialize_message

    log_file = tmp_path / "test.log"

    # Create actual messages and serialize them
    messages = [
        AssistantMessage(
            model="claude-sonnet-4",
            content=[TextBlock(text="Hello, I will help you with this task.")],
        ),
        UserMessage(content=[TextBlock(text="Great! Please proceed.")]),
        AssistantMessage(
            model="claude-sonnet-4",
            content=[TextBlock(text="I have completed the task successfully.")],
        ),
    ]

    log_content = "\n".join(serialize_message(msg) for msg in messages) + "\n"
    log_file.write_text(log_content)

    result = extract_conversation_from_log(log_file)

    # Should extract the text from TextBlock objects
    assert "Hello, I will help you with this task." in result
    assert "Great! Please proceed." in result
    assert "I have completed the task successfully." in result


def test_extract_conversation_from_log_nonexistent_file(tmp_path):
    """Test extracting from non-existent file returns empty string."""
    log_file = tmp_path / "nonexistent.log"

    result = extract_conversation_from_log(log_file)

    assert result == ""


def test_extract_conversation_from_log_empty_file(tmp_path):
    """Test extracting from empty file returns empty string."""
    log_file = tmp_path / "empty.log"
    log_file.write_text("")

    result = extract_conversation_from_log(log_file)

    assert result == ""


def test_summarize_conversation_log_no_api_key(tmp_path):
    """Test that summarization returns None when API key is missing."""
    log_file = tmp_path / "test.log"
    log_file.write_text("Some content")

    # Ensure no API key is set
    with patch.dict(os.environ, {}, clear=True):
        result = summarize_conversation_log(log_file)

    assert result is None


def test_summarize_conversation_log_with_api_key(tmp_path):
    """Test that summarization calls Anthropic API when key is present."""
    from claude_agent_sdk import AssistantMessage, UserMessage, TextBlock
    from weaver.utils import serialize_message

    log_file = tmp_path / "test.log"

    # Create actual messages and serialize them
    messages = [
        AssistantMessage(
            model="claude-sonnet-4",
            content=[TextBlock(text="I analyzed the code and found the issue.")],
        ),
        UserMessage(content=[TextBlock(text="What was the problem?")]),
        AssistantMessage(
            model="claude-sonnet-4",
            content=[TextBlock(text="The function was missing a return statement.")],
        ),
    ]
    log_content = "\n".join(serialize_message(msg) for msg in messages) + "\n"
    log_file.write_text(log_content)

    # Mock the Anthropic client
    mock_response = MagicMock()
    mock_response.content = [
        MagicMock(
            text="The agent successfully identified and fixed a missing return statement in the code."
        )
    ]

    mock_client = MagicMock()
    mock_client.messages.create.return_value = mock_response

    with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "test-key"}):
        with patch("anthropic.Anthropic", return_value=mock_client):
            result = summarize_conversation_log(log_file)

    assert (
        result
        == "The agent successfully identified and fixed a missing return statement in the code."
    )
    mock_client.messages.create.assert_called_once()

    # Verify it uses Haiku model
    call_kwargs = mock_client.messages.create.call_args[1]
    assert call_kwargs["model"] == "claude-haiku-4-5-20251001"


def test_summarize_conversation_log_api_error(tmp_path):
    """Test that summarization handles API errors gracefully."""
    from claude_agent_sdk import AssistantMessage, TextBlock
    from weaver.utils import serialize_message

    log_file = tmp_path / "test.log"

    # Create and serialize a message
    message = AssistantMessage(
        model="claude-sonnet-4",
        content=[TextBlock(text="Test content")],
    )
    log_file.write_text(serialize_message(message))

    # Mock the Anthropic client to raise an exception
    mock_client = MagicMock()
    mock_client.messages.create.side_effect = Exception("API Error")

    with patch.dict(os.environ, {"ANTHROPIC_API_KEY": "test-key"}):
        with patch('anthropic.Anthropic', return_value=mock_client):
            result = summarize_conversation_log(log_file)

    # Should return None on error
    assert result is None
