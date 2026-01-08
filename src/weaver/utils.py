"""Utility functions for Weaver."""

import json
import os
from dataclasses import asdict
from pathlib import Path
from typing import Any

from claude_agent_sdk import (
    AssistantMessage,
    SystemMessage,
    TextBlock,
    ThinkingBlock,
    ToolResultBlock,
    ToolUseBlock,
    UserMessage,
)
from rich.console import Console
from rich.panel import Panel


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


def serialize_message(
    message: AssistantMessage | UserMessage | SystemMessage | Any,
) -> str:
    """Serialize a message object to JSON for logging.

    Uses dataclasses.asdict for recursive conversion of message objects.

    Args:
        message: Message object from the Claude SDK

    Returns:
        JSON string representation of the message
    """
    return json.dumps(asdict(message))


def extract_conversation_from_log(log_file: Path) -> str:
    """Extract meaningful conversation text from agent log file.

    Args:
        log_file: Path to the log file

    Returns:
        Extracted conversation text as a string
    """
    if not log_file.exists():
        return ""

    conversation_parts = []

    with open(log_file) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            data = json.loads(line)
            # Extract text from content blocks
            # TextBlocks have a "text" field and nothing else distinctive
            for block in data.get("content", []):
                if "text" in block and len(block) == 1:
                    # This is a TextBlock (only has "text" field)
                    conversation_parts.append(block["text"])

    return "\n\n".join(conversation_parts)


def summarize_conversation_log(log_file: Path) -> str | None:
    """Summarize a conversation log to a 1-paragraph summary using Haiku.

    Args:
        log_file: Path to the agent conversation log file

    Returns:
        A 1-paragraph summary of the conversation, or None if summarization fails
    """
    # Check if API key is available
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        return None

    # Extract conversation text from log
    conversation_text = extract_conversation_from_log(log_file)
    if not conversation_text:
        return None

    try:
        from anthropic import Anthropic

        client = Anthropic(api_key=api_key)

        # Use Haiku to summarize the conversation
        response = client.messages.create(
            model="claude-haiku-4-5-20251001",
            max_tokens=500,
            messages=[
                {
                    "role": "user",
                    "content": f"""Please provide a concise 1-paragraph summary of the following AI agent conversation. Focus on what the agent accomplished, what challenges it faced, and the final outcome.

Conversation:
{conversation_text[:50000]}""",  # Limit to avoid token limits
                }
            ],
        )

        # Extract the summary text from the response
        if response.content and len(response.content) > 0:
            return response.content[0].text

    except Exception:
        # Silently fail if summarization doesn't work
        return None

    return None


def format_agent_message(message: AssistantMessage | UserMessage | SystemMessage | Any, console: Console) -> None:
    """Format and print an agent message in a pretty way.

    Args:
        message: Message object from the Claude SDK
        console: Rich Console to use for output
    """
    # Handle SystemMessage
    if isinstance(message, SystemMessage):
        if hasattr(message, "subtype") and message.subtype == "init":
            console.print("[dim]Agent session initialized[/dim]")
        return

    # Handle AssistantMessage
    if isinstance(message, AssistantMessage):
        for block in message.content:
            if isinstance(block, TextBlock):
                console.print(
                    Panel(
                        block.text,
                        title="[bold cyan]Assistant[/bold cyan]",
                        border_style="cyan",
                    )
                )

            elif isinstance(block, ThinkingBlock):
                # Optionally show thinking - could make this configurable
                pass

            elif isinstance(block, ToolUseBlock):
                # Extract useful parameters from tool input
                params = _format_tool_params(block.input)
                params_str = ", ".join(params) if params else ""
                console.print(f"[dim]→ Using tool:[/dim] [yellow]{block.name}[/yellow] {params_str}")

    # Handle UserMessage (tool results)
    elif isinstance(message, UserMessage):
        for block in message.content:
            if isinstance(block, ToolResultBlock):
                content = _format_tool_result_content(block.content)

                # Truncate long output
                lines = content.split("\n")
                if len(lines) > 10:
                    preview = "\n".join(lines[:10]) + f"\n... ({len(lines) - 10} more lines)"
                elif len(content) > 500:
                    preview = content[:500] + "..."
                else:
                    preview = content

                console.print(f"[dim]← Tool result:[/dim]\n{preview}")


def _format_tool_params(tool_input: dict[str, Any]) -> list[str]:
    """Extract useful parameters from tool input for display.

    Args:
        tool_input: Dictionary of tool input parameters

    Returns:
        List of formatted parameter strings
    """
    params = []

    # Common patterns
    if "pattern" in tool_input:
        params.append(f"pattern={tool_input['pattern']}")

    if "file_path" in tool_input:
        params.append(f"file={tool_input['file_path']}")

    if "command" in tool_input:
        cmd = tool_input["command"].replace("\n", " ")
        if len(cmd) > 60:
            cmd = cmd[:60] + "..."
        params.append(f"cmd={cmd}")

    if "prompt" in tool_input:
        prompt = tool_input["prompt"].replace("\n", " ")
        if len(prompt) > 60:
            prompt = prompt[:60] + "..."
        params.append(f"prompt={prompt}")

    return params


def _format_tool_result_content(content: Any) -> str:
    """Format tool result content for display.

    Args:
        content: Content from a ToolResultBlock (can be str, list, or None)

    Returns:
        Formatted content as a string
    """
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        # Handle list of content blocks
        parts = []
        for item in content:
            if isinstance(item, dict):
                if item.get("type") == "text":
                    parts.append(item.get("text", ""))
            else:
                parts.append(str(item))
        return "\n".join(parts)
    return str(content)
