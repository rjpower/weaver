#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["click>=8.1"]
# ///
"""Builds the loom image locally and pushes it to this project's Artifact
Registry, so a VM can boot with `bootstrap.py --image-mode=pull` instead of
building the (slow, RAM-hungry) image on the VM itself. Run from your
workstation. See ./README.md "Build once, pull many".

The Artifact Registry path is derived from --project/--region (a fixed
`loom/loom` repo), the same path `bootstrap.py --image-mode=pull` defaults to —
so once you've pushed, you never type the image path by hand. This creates the
AR repo and wires `docker` auth on first run (both check-before-create).

Builds for linux/amd64 (the GCE VM's arch) via buildx. On a non-amd64
workstation that needs an emulating `docker-container` builder
(`docker buildx create --use`); a native amd64 host is fine as-is.
"""

import subprocess
import sys
import tomllib
from pathlib import Path

import click

# Repo root (holds the Dockerfile): deploy/gcp/push-image.py -> ../../.
REPO_ROOT = Path(__file__).resolve().parents[2]
DEPLOY_TOML = Path(__file__).resolve().parent / "deploy.toml"


def log(msg: str) -> None:
    click.echo(f"▶ {msg}", err=True)


def die(msg: str) -> None:
    click.echo(msg, err=True)
    sys.exit(1)


def load_deploy_toml() -> dict:
    """Reuse bootstrap.py's remembered defaults (project, region) as click
    default_map, so you don't re-type them here either."""
    if not DEPLOY_TOML.exists():
        return {}
    with DEPLOY_TOML.open("rb") as f:
        return tomllib.load(f)


def gcloud(project: str, *args: str) -> None:
    subprocess.run(["gcloud", f"--project={project}", *args], check=True)


def gcloud_exists(project: str, *args: str) -> bool:
    return (
        subprocess.run(
            ["gcloud", f"--project={project}", *args],
            capture_output=True,
            text=True,
            stdin=subprocess.DEVNULL,
        ).returncode
        == 0
    )


def default_ar_image(project: str, region: str, repo: str, tag: str) -> str:
    """Keep the `loom/loom` shape in sync with bootstrap.py's default_ar_image."""
    return f"{region}-docker.pkg.dev/{project}/{repo}/loom:{tag}"


@click.command(context_settings={"help_option_names": ["-h", "--help"]})
@click.option("--project", envvar="PROJECT", required=True, help="GCP project id.")
@click.option("--region", envvar="REGION", default="us-central1", show_default=True)
@click.option(
    "--repo",
    envvar="AR_REPO",
    default="loom",
    show_default=True,
    help="Artifact Registry docker repository (created if absent).",
)
@click.option("--tag", default="latest", show_default=True, help="Image tag.")
@click.option(
    "--profile",
    type=click.Choice(["release", "debug"]),
    default="release",
    show_default=True,
    help="cargo profile baked into the pushed image (CARGO_PROFILE build arg). "
    "release for a real deploy; debug only to push a fast throwaway.",
)
@click.option(
    "--platform",
    default="linux/amd64",
    show_default=True,
    help="Target arch — the GCE VM is amd64; only change if you deploy elsewhere.",
)
def main(
    project: str, region: str, repo: str, tag: str, profile: str, platform: str
) -> None:
    ar_host = f"{region}-docker.pkg.dev"
    image = default_ar_image(project, region, repo, tag)

    log("enabling Artifact Registry API")
    gcloud(project, "services", "enable", "artifactregistry.googleapis.com")

    if not gcloud_exists(
        project, "artifacts", "repositories", "describe", repo, f"--location={region}"
    ):
        log(f"creating Artifact Registry repo {repo} in {region}")
        gcloud(
            project,
            "artifacts",
            "repositories",
            "create",
            repo,
            "--repository-format=docker",
            f"--location={region}",
        )

    # Idempotent: adds ar_host to ~/.docker/config.json's credHelpers if absent.
    log(f"configuring docker auth for {ar_host}")
    gcloud(project, "auth", "configure-docker", ar_host, "--quiet")

    log(f"building + pushing {image}  (profile={profile}, platform={platform})")
    subprocess.run(
        [
            "docker",
            "buildx",
            "build",
            f"--platform={platform}",
            f"--build-arg=CARGO_PROFILE={profile}",
            f"--tag={image}",
            "--push",
            str(REPO_ROOT),
        ],
        check=True,
    )

    click.echo("", err=True)
    log(f"pushed {image}")
    log("boot a VM from it with:")
    log(f"  ./bootstrap.py --project={project} --image-mode=pull")
    log("(--ar-image defaults to exactly this path, so it's left off.)")


if __name__ == "__main__":
    main(default_map=load_deploy_toml())
