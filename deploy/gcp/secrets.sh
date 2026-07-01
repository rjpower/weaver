#!/usr/bin/env bash
#
# Creates/updates the Secret Manager secrets startup-script.sh reads on boot.
# Run from your workstation, any time before or after bootstrap.sh (the VM's
# service account can read Secret Manager regardless of instance state) —
# and again whenever you need to rotate a value.
#
# Usage:
#   PROJECT=my-project ./secrets.sh                  # prompts for every secret
#   PROJECT=my-project ./secrets.sh GH_TOKEN          # just one
#   PROJECT=my-project GH_TOKEN=ghp_xxx ./secrets.sh  # non-interactive: reads
#                                                      # from an already-exported
#                                                      # env var of the same name
#
# Values are never echoed and never appear as a process argument: interactive
# entry uses `read -s`, and both paths pipe the value to gcloud over stdin.
set -euo pipefail

: "${PROJECT:?set PROJECT to your GCP project id}"

SECRET_NAMES=(
  GH_TOKEN
  ANTHROPIC_API_KEY
  LOOM_GITHUB_WEBHOOK_SECRET
  LOOM_GITHUB_CLIENT_ID
  LOOM_GITHUB_CLIENT_SECRET
  LOOM_OWNER_GITHUB
  LOOM_DOMAIN
)

names=("$@")
if [ "${#names[@]}" -eq 0 ]; then
  names=("${SECRET_NAMES[@]}")
fi

for name in "${names[@]}"; do
  found=0
  for known in "${SECRET_NAMES[@]}"; do
    [ "$name" = "$known" ] && found=1 && break
  done
  if [ "$found" -eq 0 ]; then
    echo "unknown secret name: $name (expected one of: ${SECRET_NAMES[*]})" >&2
    exit 1
  fi
done

gcp() { gcloud --project="$PROJECT" "$@"; }

for name in "${names[@]}"; do
  value="${!name:-}"
  if [ -z "$value" ]; then
    read -r -s -p "value for ${name}: " value
    echo >&2
  fi
  if [ -z "$value" ]; then
    echo "empty value for ${name}, skipping" >&2
    continue
  fi

  if ! gcp secrets describe "$name" >/dev/null 2>&1; then
    echo "▶ creating secret $name" >&2
    gcp secrets create "$name" --replication-policy=automatic >/dev/null
  fi
  printf '%s' "$value" | gcp secrets versions add "$name" --data-file=- >/dev/null
  echo "▶ set $name" >&2
  unset value
done
