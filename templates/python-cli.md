---
name: Python CLI
description: Best practices for building command-line interfaces with Click
category: coding
---

# Python CLI Template

Guide for building well-structured command-line interfaces using Click.

## 1. Project Setup

- [ ] Add `click` to project dependencies
- [ ] Create entry point in `pyproject.toml` under `[project.scripts]`
- [ ] Organize CLI code in a dedicated module (e.g., `cli.py` or `cli/` package)

Example `pyproject.toml` entry:

```toml
[project.scripts]
mytool = "mypackage.cli:main"
```

## 2. Basic Command Structure

- [ ] Use `@click.command()` for single commands
- [ ] Use `@click.group()` for multi-command CLIs
- [ ] Keep command functions focused on CLI concerns; delegate logic to library code

```python
import click

@click.group()
def main() -> None:
    """My tool description shown in --help."""
    pass

@main.command()
@click.argument("name")
def greet(name: str) -> None:
    """Greet a user by name."""
    click.echo(f"Hello, {name}!")

if __name__ == "__main__":
    main()
```

## 3. Arguments and Options

- [ ] Use `@click.argument()` for required positional inputs
- [ ] Use `@click.option()` for optional flags and named parameters
- [ ] Provide `--help` text via docstrings and `help=` parameters
- [ ] Use appropriate types (`click.Path`, `click.Choice`, `click.INT`, etc.)

```python
@main.command()
@click.argument("input_file", type=click.Path(exists=True))
@click.option("--output", "-o", type=click.Path(), help="Output file path.")
@click.option("--verbose", "-v", is_flag=True, help="Enable verbose output.")
@click.option(
    "--format",
    type=click.Choice(["json", "csv", "text"]),
    default="text",
    help="Output format.",
)
def process(input_file: str, output: str | None, verbose: bool, format: str) -> None:
    """Process INPUT_FILE and write results."""
    ...
```

## 4. Help Text Best Practices

- [ ] Write clear, concise docstrings for commands (shown in `--help`)
- [ ] Document every option with the `help=` parameter
- [ ] Use uppercase for argument metavars (e.g., `INPUT_FILE`)
- [ ] Show examples in command docstrings when behavior is non-obvious

```python
@main.command()
@click.argument("pattern")
@click.argument("paths", nargs=-1, type=click.Path(exists=True))
def search(pattern: str, paths: tuple[str, ...]) -> None:
    """Search for PATTERN in one or more PATHS.

    Examples:

        mytool search "error" logs/
        mytool search "TODO" src/ tests/
    """
    ...
```

## 5. Error Handling

Don't swallow exceptions, let stack traces propagate.

## 6. Output Conventions

- [ ] Use `click.echo()` for normal output (handles encoding properly)
- [ ] Use `click.secho()` for colored output when appropriate
- [ ] Write to stderr for progress/status messages (`err=True`)
- [ ] Support `--quiet` flag for scriptable output

```python
@main.command()
@click.option("--quiet", "-q", is_flag=True, help="Suppress status messages.")
def build(quiet: bool) -> None:
    """Build the project."""
    if not quiet:
        click.echo("Building...", err=True)
    result = do_build()
    click.echo(result.output)  # Actual output to stdout
```

## 7. Subcommand Organization

- [ ] Group related commands under subgroups for complex CLIs
- [ ] Use consistent naming (verbs for actions: `create`, `list`, `delete`)
- [ ] Share common options via decorators or context

```python
@main.group()
def users() -> None:
    """Manage users."""
    pass

@users.command("list")
def list_users() -> None:
    """List all users."""
    ...

@users.command()
@click.argument("username")
def create(username: str) -> None:
    """Create a new user."""
    ...
```

## 8. Testing CLI Commands

- [ ] Use `click.testing.CliRunner` for testing
- [ ] Test both success and error cases
- [ ] Verify exit codes and output content

```python
from click.testing import CliRunner
from mypackage.cli import main

def test_greet() -> None:
    runner = CliRunner()
    result = runner.invoke(main, ["greet", "World"])
    assert result.exit_code == 0
    assert "Hello, World!" in result.output

def test_missing_argument() -> None:
    runner = CliRunner()
    result = runner.invoke(main, ["greet"])
    assert result.exit_code != 0
    assert "Missing argument" in result.output
```

## 9. Configuration and Context

- [ ] Use `click.Context` to pass shared state between commands
- [ ] Support configuration files for complex CLIs
- [ ] Follow precedence: CLI args > env vars > config file > defaults

```python
@main.command()
@click.option("--config", type=click.Path(exists=True), envvar="MYTOOL_CONFIG")
@click.pass_context
def run(ctx: click.Context, config: str | None) -> None:
    """Run with optional config file."""
    settings = load_settings(config)
    ctx.obj = settings
    ...
```

## 10. Checklist Summary

- [ ] Entry point defined in `pyproject.toml`
- [ ] All commands have docstrings for `--help`
- [ ] All options have `help=` text
- [ ] Arguments use appropriate `click.Path`/`click.Choice` types
- [ ] Errors raise `click.ClickException` with clear messages
- [ ] Tests use `CliRunner` and check exit codes
- [ ] Complex CLIs use subgroups for organization
