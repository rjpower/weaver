"""Utility functions for Weaver."""


def truncate_content(text: str, max_words: int = 200) -> tuple[str, bool]:
    """Truncate text to max_words.

    Splits text on whitespace and truncates to the specified number of words,
    preserving word boundaries.

    Args:
        text: The text to truncate
        max_words: Maximum number of words to keep (default: 200)

    Returns:
        Tuple of (truncated_text, was_truncated) where:
        - truncated_text: The text, possibly truncated with "..." appended
        - was_truncated: True if text was truncated, False otherwise
    """
    words = text.split()

    if len(words) <= max_words:
        return (text, False)

    truncated = " ".join(words[:max_words]) + "..."
    return (truncated, True)
