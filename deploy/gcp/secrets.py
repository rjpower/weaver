#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["click>=8.1"]
# ///
"""Creates/updates the Secret Manager secrets startup-script.sh reads on boot.
Run from your workstation, any time before or after bootstrap.py (the VM's
service account can read Secret Manager regardless of instance state) — and
again whenever you need to rotate a value.

`deploy/standalone/.env` is the single handoff contract between `loom setup`
(which writes it) and this deploy: the default mode reads the secret values
straight out of it instead of re-prompting for credentials the wizard already
collected.

Usage:
  PROJECT=my-project ./secrets.py
      # ingests ../standalone/.env if it exists (the `loom setup` output),
      # else prompts for every secret.

  PROJECT=my-project ./secrets.py --from-env /path/to/.env
      # ingests a specific .env file.

  PROJECT=my-project ./secrets.py GH_TOKEN
      # just one, prompted (or read from --from-env if given).

  PROJECT=my-project GH_TOKEN=ghp_xxx ./secrets.py
      # non-interactive: reads from an already-exported env var of the same
      # name. Exported env vars always win over a --from-env file, so this
      # also works to override one value while ingesting the rest from file.

Values are never echoed and never appear as a process argument: interactive
entry uses a hidden prompt (a visible multi-line paste for the RSA private
key, which can't be read as a single hidden line), and all paths pipe the
value to gcloud over stdin.
"""

import os
import subprocess
import sys
from pathlib import Path

import click

# Exact key contract `loom setup` writes to deploy/standalone/.env — see
# ../standalone/.env.example and crates/loom/src/envfile.rs.
SECRET_NAMES = [
    "LOOM_GITHUB_APP_ID",
    "LOOM_GITHUB_APP_PRIVATE_KEY",
    "LOOM_GITHUB_WEBHOOK_SECRET",
    "LOOM_GITHUB_CLIENT_ID",
    "LOOM_GITHUB_CLIENT_SECRET",
    "ANTHROPIC_API_KEY",
    "GH_TOKEN",
    "LOOM_DOMAIN",
    "LOOM_OWNER_GITHUB",
]

# Values that are pasted, not typed, because they're multi-line.
MULTILINE_NAMES = {"LOOM_GITHUB_APP_PRIVATE_KEY"}

DEFAULT_ENV_PATH = Path(__file__).resolve().parent / ".." / "standalone" / ".env"


def gcloud_exists(project: str, *args: str) -> bool:
    result = subprocess.run(
        ["gcloud", f"--project={project}", *args],
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
    )
    return result.returncode == 0


def gcloud(project: str, *args: str, input_text: str | None = None) -> None:
    # stdin=DEVNULL when there's no input to send: subprocess otherwise
    # inherits the parent's stdin, which can hang forever if it's a pipe
    # that never closes (e.g. this script itself run non-interactively).
    subprocess.run(
        ["gcloud", f"--project={project}", *args],
        check=True,
        input=input_text,
        text=True,
        capture_output=True,
        stdin=subprocess.DEVNULL if input_text is None else None,
    )


def unformat_value(raw: str) -> str:
    r"""Reverse of envfile.rs::format_value: unescape a dotenv value back to its
    raw form. Bare values pass through unchanged; a double-quoted value has
    escaped-backslash, escaped-quote, and escaped-newline sequences unescaped
    back to a literal backslash, quote, and real newline, in that left-to-right
    order — matching how format_value produced them, so a two-character
    newline escape and a two-character backslash escape can never collide.
    """
    raw = raw.strip()
    if len(raw) < 2 or raw[0] != '"' or raw[-1] != '"':
        return raw
    inner = raw[1:-1]
    out: list[str] = []
    i = 0
    while i < len(inner):
        c = inner[i]
        if c == "\\" and i + 1 < len(inner):
            nxt = inner[i + 1]
            if nxt == "n":
                out.append("\n")
                i += 2
                continue
            if nxt in ('"', "\\"):
                out.append(nxt)
                i += 2
                continue
        out.append(c)
        i += 1
    return "".join(out)


def parse_dotenv(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in path.read_text().splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in line:
            continue
        key, _, raw_value = line.partition("=")
        values[key.strip()] = unformat_value(raw_value)
    return values


def prompt_multiline(name: str) -> str:
    click.echo(f"paste value for {name}, then press Ctrl-D on an empty line:", err=True)
    return sys.stdin.read().strip("\n")


@click.command(
    context_settings={"help_option_names": ["-h", "--help"]},
    help="Create/update secrets. NAMES defaults to all of: " + ", ".join(SECRET_NAMES),
)
@click.option("--project", envvar="PROJECT", required=True, help="GCP project id.")
@click.option(
    "--from-env",
    "from_env",
    type=click.Path(path_type=Path, exists=True, dir_okay=False),
    default=None,
    help="Ingest secret values from an existing dotenv file (e.g. "
    "deploy/standalone/.env, as written by `loom setup`) instead of "
    "prompting. Defaults to ../standalone/.env when it exists and no NAMES "
    "are given. An exported env var of the same name still overrides a "
    "value from this file.",
)
@click.argument("names", nargs=-1)
def main(project: str, from_env: Path | None, names: tuple[str, ...]) -> None:
    selected = list(names) or SECRET_NAMES

    for name in selected:
        if name not in SECRET_NAMES:
            click.echo(
                f"unknown secret name: {name} (expected one of: {', '.join(SECRET_NAMES)})",
                err=True,
            )
            sys.exit(1)

    if from_env is None and not names and DEFAULT_ENV_PATH.exists():
        from_env = DEFAULT_ENV_PATH

    env_values: dict[str, str] = {}
    if from_env is not None:
        click.echo(f"▶ ingesting secrets from {from_env}", err=True)
        env_values = parse_dotenv(from_env)

    for name in selected:
        value = os.environ.get(name, "") or env_values.get(name, "")
        if not value:
            if name in MULTILINE_NAMES:
                value = prompt_multiline(name)
            else:
                value = click.prompt(
                    f"value for {name}", hide_input=True, default="", show_default=False
                )
        if not value:
            click.echo(f"empty value for {name}, skipping", err=True)
            continue

        if not gcloud_exists(project, "secrets", "describe", name):
            click.echo(f"▶ creating secret {name}", err=True)
            gcloud(project, "secrets", "create", name, "--replication-policy=automatic")
        gcloud(
            project,
            "secrets",
            "versions",
            "add",
            name,
            "--data-file=-",
            input_text=value,
        )
        click.echo(f"▶ set {name}", err=True)
        del value


if __name__ == "__main__":
    main()
