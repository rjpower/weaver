---
name: Python Coding
description: Python coding standards and best practices
category: coding
---

# Python Coding Template

Standards and conventions for writing maintainable Python code.

## 1. Tooling

- [ ] Target Python >= 3.11
- [ ] Use `uv` for dependency management and running scripts
- [ ] Run `uv run` for all Python entry points
- [ ] Use `ruff` for formatting and linting
- [ ] Maintain passing type hints under `mypy`

## 2. Code Style

### Imports

- [ ] Place all imports at the top of the file
- [ ] Avoid local imports unless technically necessary (circular dependencies, optional dependencies)
- [ ] Group imports: standard library, third-party, local (separated by blank lines)

```python
import os
from pathlib import Path

import click
import httpx

from mypackage.core import process
from mypackage.utils import helpers
```

### Functions vs Classes

- [ ] Prefer top-level functions when code does not mutate shared state
- [ ] Use classes to encapsulate data when it improves clarity
- [ ] Avoid classes that are just bags of static methods

```python
# Prefer: top-level function for stateless operations
def process_items(items: list[Item]) -> list[Result]:
    return [transform(item) for item in items]

# Use class when managing state
class ItemProcessor:
    def __init__(self, config: Config) -> None:
        self.config = config
        self._cache: dict[str, Result] = {}

    def process(self, item: Item) -> Result:
        ...
```

### Control Flow

- [ ] Use early returns to reduce nesting

```python
# Prefer
def get_user(user_id: str) -> User | None:
    if not user_id:
        return None
    user = db.fetch(user_id)
    if not user.is_active:
        return None
    return user

# Avoid
def get_user(user_id: str) -> User | None:
    if user_id:
        user = db.fetch(user_id)
        if user.is_active:
            return user
    return None
```

### Avoid Compatibility Hacks

- [ ] Do not use `hasattr()` for feature detection; use Protocols or update calling code
- [ ] Do not introduce deprecation or fallback paths; update all call sites instead
- [ ] Do not use `from __future__ import ...` statements

## 3. Error Handling

- [ ] Let exceptions propagate by default
- [ ] Only catch exceptions when you can add meaningful context and re-raise
- [ ] Only catch exceptions when intentionally altering control flow
- [ ] Never swallow exceptions silently

```python
# Good: add context and re-raise
def load_config(path: Path) -> Config:
    try:
        data = path.read_text()
    except OSError as e:
        raise ConfigError(f"Failed to read config from {path}") from e
    return parse_config(data)

# Good: intentionally handling the exception
def get_cached_value(key: str) -> Value | None:
    try:
        return cache.get(key)
    except CacheMiss:
        return None

# Bad: swallowing exceptions
def process(data: str) -> Result:
    try:
        return do_process(data)
    except Exception:
        pass  # Never do this
```

## 4. Type Hints

- [ ] Add type hints to all function signatures
- [ ] Use modern syntax: `list[str]` not `List[str]`, `str | None` not `Optional[str]`
- [ ] Use `TypedDict` for structured dictionaries
- [ ] Use `Protocol` for structural typing instead of ABCs when appropriate

```python
from typing import Protocol, TypedDict

class Processor(Protocol):
    def process(self, data: bytes) -> bytes: ...

class UserData(TypedDict):
    id: str
    name: str
    email: str | None

def handle_user(data: UserData, processor: Processor) -> bytes:
    ...
```

## 5. Testing

- [ ] Use pytest for all tests
- [ ] Prefer top-level test functions over test classes
- [ ] Use fixtures for shared setup
- [ ] Use `pytest.mark.parametrize` to avoid duplication
- [ ] Always fix tests if you broke them; do not relax tolerances or hack around failures

```python
import pytest
from mypackage import process

@pytest.fixture
def sample_input() -> Input:
    return Input(data="test")

def test_process_basic(sample_input: Input) -> None:
    result = process(sample_input)
    assert result.status == "success"

@pytest.mark.parametrize("value,expected", [
    ("a", 1),
    ("bb", 2),
    ("ccc", 3),
])
def test_length(value: str, expected: int) -> None:
    assert len(value) == expected
```

## 6. Documentation

### Docstrings

- [ ] Use Google-style docstrings for public APIs
- [ ] Keep docstrings concise and focused on behavior
- [ ] Do not write comments that merely restate the code

```python
def fetch_records(
    query: str,
    limit: int = 100,
) -> list[Record]:
    """Fetch records matching the query.

    Args:
        query: SQL-like query string.
        limit: Maximum records to return.

    Returns:
        List of matching records, ordered by relevance.

    Raises:
        QueryError: If the query syntax is invalid.
    """
    ...
```

### Comments

- [ ] Write comments to explain *why*, not *what*
- [ ] Use comments for module-level or class-level context
- [ ] Use comments to explain subtle or non-obvious behavior

```python
# Bad: restates the code
# Create an in-memory cache
cache = InMemoryCache()

# Good: explains why
# We use an in-memory cache here because the data is small (<10MB)
# and Redis adds unnecessary latency for this use case.
cache = InMemoryCache()
```

## 7. File and Data Access

- [ ] Use `pathlib.Path` for file paths
- [ ] Use `fsspec` for filesystem-agnostic access when supporting remote storage
- [ ] Stream large files instead of loading into memory
- [ ] Avoid hard-coding absolute paths

```python
from pathlib import Path

def read_config(path: Path) -> dict:
    return json.loads(path.read_text())

# For remote/local agnostic access
import fsspec

def read_remote(uri: str) -> bytes:
    with fsspec.open(uri, "rb") as f:
        return f.read()
```

## 8. Checklist Summary

- [ ] All imports at top of file, properly grouped
- [ ] Functions preferred over classes for stateless operations
- [ ] Early returns used to reduce nesting
- [ ] Exceptions propagate unless explicitly handled with context
- [ ] Type hints on all function signatures
- [ ] Tests use pytest fixtures and parametrization
- [ ] Public APIs have Google-style docstrings
- [ ] Comments explain *why*, not *what*
