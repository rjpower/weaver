#!/usr/bin/env bash
#
# Provisions a single GCE VM that runs the standalone loom stack
# (../standalone/docker-compose.yml) behind its bundled Caddy front-door. Run
# this from your workstation, not the VM. See ./README.md for the full runbook
# and the required run order.
#
# This is Model 1 (single host) only — see ../README.md "Future: cloud /
# cluster". It does not provision a cluster, Cloud Run, or per-session
# isolation.
#
# Every gcloud call here is check-before-create, so re-running after a partial
# failure (or to change a knob) is safe.
#
# Configuration is via environment variables, all optional except PROJECT and
# LOOM_DOMAIN:
#   PROJECT            GCP project id. No default — required.
#   LOOM_DOMAIN         Public domain the VM will serve. No default — required.
#   REGION              Default: us-central1
#   ZONE                Default: ${REGION}-a
#   MACHINE_TYPE        Default: e2-standard-4
#   DISK_SIZE           Boot disk size, GB. Default: 100
#   DATA_DISK_SIZE      Persistent data-disk size, GB, for loom_home/caddy_data
#                        (see ../README.md "Durable state"). Default: 50.
#                        Set to 0 to skip — state then lives on the boot disk.
#   INSTANCE_NAME       Default: loom
#   SERVICE_ACCOUNT_NAME Default: loom-vm — a dedicated, low-privilege SA
#                        (granted only secretmanager.secretAccessor).
#   NETWORK              Default: default
#   OPERATOR_IP          Default: auto-detected public IP (used for the SSH
#                        firewall rule's /32). Override if you SSH from a
#                        different network than you run this script from.
#   REPO_URL             Git URL the VM clones to get deploy/standalone.
#                        Default: this repo's origin remote.
#   GIT_REF              Branch/tag/ref to check out. Default: main
#   IMAGE_MODE            "build" (default) — VM builds the image itself with
#                        `docker compose up -d --build` (slow, needs the roomy
#                        default machine). "pull" — VM pulls a prebuilt image
#                        from Artifact Registry (AR_IMAGE required); see
#                        ../README.md "Build once, pull many".
#   AR_IMAGE              Required when IMAGE_MODE=pull, e.g.
#                        us-central1-docker.pkg.dev/$PROJECT/loom/loom:latest
#   DNS_WAIT_SECONDS      How long to poll for the DNS record before asking
#                        whether to proceed anyway. Default: 600
#   SKIP_DNS_WAIT          Set to 1 to skip the DNS wait/confirmation gate
#                        entirely (e.g. re-running bootstrap.sh against an
#                        already-live domain).
#
# Secrets (GH_TOKEN, ANTHROPIC_API_KEY, ...) are NOT handled here — run
# ./secrets.sh first (or after; order doesn't matter, see ../README.md).
set -euo pipefail

: "${PROJECT:?set PROJECT to your GCP project id}"
: "${LOOM_DOMAIN:?set LOOM_DOMAIN to the public domain this VM will serve}"

REGION="${REGION:-us-central1}"
ZONE="${ZONE:-${REGION}-a}"
MACHINE_TYPE="${MACHINE_TYPE:-e2-standard-4}"
DISK_SIZE="${DISK_SIZE:-100}"
DATA_DISK_SIZE="${DATA_DISK_SIZE:-50}"
INSTANCE_NAME="${INSTANCE_NAME:-loom}"
SERVICE_ACCOUNT_NAME="${SERVICE_ACCOUNT_NAME:-loom-vm}"
NETWORK="${NETWORK:-default}"
REPO_URL="${REPO_URL:-https://github.com/rjpower/weaver.git}"
GIT_REF="${GIT_REF:-main}"
IMAGE_MODE="${IMAGE_MODE:-build}"
AR_IMAGE="${AR_IMAGE:-}"
DNS_WAIT_SECONDS="${DNS_WAIT_SECONDS:-600}"
SKIP_DNS_WAIT="${SKIP_DNS_WAIT:-0}"

SA_EMAIL="${SERVICE_ACCOUNT_NAME}@${PROJECT}.iam.gserviceaccount.com"
IP_NAME="${INSTANCE_NAME}-ip"
DATA_DISK_NAME="${INSTANCE_NAME}-data"
DATA_DISK_DEVICE="loom-data"
FW_WEB="${INSTANCE_NAME}-allow-web"
FW_SSH="${INSTANCE_NAME}-allow-ssh"

log() { printf '▶ %s\n' "$*" >&2; }
warn() { printf '⚠ %s\n' "$*" >&2; }

if [ "$IMAGE_MODE" = "pull" ] && [ -z "$AR_IMAGE" ]; then
  echo "IMAGE_MODE=pull requires AR_IMAGE (e.g. us-central1-docker.pkg.dev/$PROJECT/loom/loom:latest)" >&2
  exit 1
fi

gcp() { gcloud --project="$PROJECT" "$@"; }

enable_apis() {
  log "enabling required APIs"
  local apis=(compute.googleapis.com secretmanager.googleapis.com)
  if [ "$IMAGE_MODE" = "pull" ]; then
    apis+=(artifactregistry.googleapis.com)
  fi
  gcp services enable "${apis[@]}"
}

ensure_service_account() {
  log "ensuring service account $SA_EMAIL"
  if ! gcp iam service-accounts describe "$SA_EMAIL" >/dev/null 2>&1; then
    gcp iam service-accounts create "$SERVICE_ACCOUNT_NAME" \
      --display-name="loom standalone VM"
  fi
  # Least-privilege: only Secret Manager read access. The startup script uses
  # this identity (via the metadata server's token endpoint, through gcloud)
  # to fetch the secrets it writes into deploy/standalone/.env.
  gcp projects add-iam-policy-binding "$PROJECT" \
    --member="serviceAccount:${SA_EMAIL}" \
    --role="roles/secretmanager.secretAccessor" \
    --condition=None >/dev/null
  if [ "$IMAGE_MODE" = "pull" ]; then
    gcp projects add-iam-policy-binding "$PROJECT" \
      --member="serviceAccount:${SA_EMAIL}" \
      --role="roles/artifactregistry.reader" \
      --condition=None >/dev/null
  fi
}

ensure_firewall() {
  local operator_ip="${OPERATOR_IP:-}"
  if [ -z "$operator_ip" ]; then
    log "auto-detecting operator public IP"
    operator_ip="$(curl -sf https://api.ipify.org || true)"
  fi
  if [ -z "$operator_ip" ]; then
    echo "could not auto-detect your public IP; set OPERATOR_IP explicitly" >&2
    exit 1
  fi

  log "ensuring firewall rule $FW_WEB (tcp:80,tcp:443,udp:443 from 0.0.0.0/0)"
  if ! gcp compute firewall-rules describe "$FW_WEB" >/dev/null 2>&1; then
    gcp compute firewall-rules create "$FW_WEB" \
      --network="$NETWORK" \
      --direction=INGRESS \
      --action=ALLOW \
      --rules=tcp:80,tcp:443,udp:443 \
      --source-ranges=0.0.0.0/0 \
      --target-tags=loom-web
  fi

  log "ensuring firewall rule $FW_SSH (tcp:22 from ${operator_ip}/32 only)"
  if ! gcp compute firewall-rules describe "$FW_SSH" >/dev/null 2>&1; then
    gcp compute firewall-rules create "$FW_SSH" \
      --network="$NETWORK" \
      --direction=INGRESS \
      --action=ALLOW \
      --rules=tcp:22 \
      --source-ranges="${operator_ip}/32" \
      --target-tags=loom-ssh
  else
    gcp compute firewall-rules update "$FW_SSH" \
      --source-ranges="${operator_ip}/32"
  fi
  # 7878 (loom's own port) is intentionally never opened here — the only way
  # in is through Caddy on 80/443. See ../standalone/docker-compose.yml.
}

ensure_static_ip() {
  log "ensuring static external IP $IP_NAME"
  if ! gcp compute addresses describe "$IP_NAME" --region="$REGION" >/dev/null 2>&1; then
    gcp compute addresses create "$IP_NAME" --region="$REGION"
  fi
  gcp compute addresses describe "$IP_NAME" --region="$REGION" --format='value(address)'
}

wait_for_dns() {
  local ip="$1"
  echo >&2
  echo "═══════════════════════════════════════════════════════════════════" >&2
  echo "  Set this DNS record before continuing (ACME HTTP-01 needs it to" >&2
  echo "  resolve BEFORE the stack starts, or the TLS certificate won't issue):" >&2
  echo >&2
  echo "    ${LOOM_DOMAIN}.   A   ${ip}" >&2
  echo >&2
  echo "═══════════════════════════════════════════════════════════════════" >&2

  if [ "$SKIP_DNS_WAIT" = "1" ]; then
    warn "SKIP_DNS_WAIT=1 — not waiting for DNS to resolve"
    return
  fi

  resolve() {
    if command -v dig >/dev/null 2>&1; then
      dig +short A "$LOOM_DOMAIN" | tail -n1
    elif command -v host >/dev/null 2>&1; then
      host -t A "$LOOM_DOMAIN" 2>/dev/null | awk '/has address/{print $NF; exit}'
    else
      python3 -c "import socket,sys
try:
    print(socket.gethostbyname(sys.argv[1]))
except OSError:
    pass" "$LOOM_DOMAIN"
    fi
  }

  log "waiting up to ${DNS_WAIT_SECONDS}s for ${LOOM_DOMAIN} to resolve to ${ip}"
  local waited=0
  while [ "$waited" -lt "$DNS_WAIT_SECONDS" ]; do
    if [ "$(resolve)" = "$ip" ]; then
      log "${LOOM_DOMAIN} resolves to ${ip}"
      return
    fi
    sleep 15
    waited=$((waited + 15))
  done

  warn "${LOOM_DOMAIN} does not resolve to ${ip} yet after ${DNS_WAIT_SECONDS}s"
  read -r -p "Continue and create the VM anyway? [y/N] " reply
  case "$reply" in
    [yY]*) ;;
    *) echo "aborting; re-run bootstrap.sh once DNS is set" >&2; exit 1 ;;
  esac
}

ensure_data_disk() {
  [ "$DATA_DISK_SIZE" -gt 0 ] || return 0
  log "ensuring data disk $DATA_DISK_NAME (${DATA_DISK_SIZE}GB)"
  if ! gcp compute disks describe "$DATA_DISK_NAME" --zone="$ZONE" >/dev/null 2>&1; then
    gcp compute disks create "$DATA_DISK_NAME" \
      --zone="$ZONE" \
      --size="${DATA_DISK_SIZE}GB" \
      --type=pd-balanced
  fi
}

ensure_instance() {
  log "ensuring instance $INSTANCE_NAME"
  if gcp compute instances describe "$INSTANCE_NAME" --zone="$ZONE" >/dev/null 2>&1; then
    log "instance $INSTANCE_NAME already exists — leaving it as-is"
    log "(delete it first if you want bootstrap.sh to recreate it with new settings)"
    return
  fi

  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

  local metadata="loom-domain=${LOOM_DOMAIN},repo-url=${REPO_URL},git-ref=${GIT_REF},image-mode=${IMAGE_MODE}"
  if [ -n "$AR_IMAGE" ]; then
    metadata="${metadata},ar-image=${AR_IMAGE}"
  fi

  # A possibly-empty array expanded with "${arr[@]}" trips "unbound variable"
  # under set -u on bash <4.4 (e.g. macOS's system /bin/bash 3.2), so branch on
  # a full command instead of splicing an optional array element in.
  if [ "$DATA_DISK_SIZE" -gt 0 ]; then
    gcp compute instances create "$INSTANCE_NAME" \
      --zone="$ZONE" \
      --machine-type="$MACHINE_TYPE" \
      --image-family=debian-12 \
      --image-project=debian-cloud \
      --boot-disk-size="${DISK_SIZE}GB" \
      --boot-disk-type=pd-balanced \
      --tags=loom-web,loom-ssh \
      --service-account="$SA_EMAIL" \
      --scopes=cloud-platform \
      --address="$IP_NAME" \
      --metadata="$metadata" \
      --metadata-from-file="startup-script=${script_dir}/startup-script.sh" \
      --disk="name=${DATA_DISK_NAME},device-name=${DATA_DISK_DEVICE},mode=rw,boot=no"
  else
    gcp compute instances create "$INSTANCE_NAME" \
      --zone="$ZONE" \
      --machine-type="$MACHINE_TYPE" \
      --image-family=debian-12 \
      --image-project=debian-cloud \
      --boot-disk-size="${DISK_SIZE}GB" \
      --boot-disk-type=pd-balanced \
      --tags=loom-web,loom-ssh \
      --service-account="$SA_EMAIL" \
      --scopes=cloud-platform \
      --address="$IP_NAME" \
      --metadata="$metadata" \
      --metadata-from-file="startup-script=${script_dir}/startup-script.sh"
  fi
}

main() {
  enable_apis
  ensure_service_account
  ensure_firewall
  local ip
  ip="$(ensure_static_ip)"
  wait_for_dns "$ip"
  ensure_data_disk
  ensure_instance

  echo >&2
  log "done. VM: $INSTANCE_NAME  IP: $ip  domain: $LOOM_DOMAIN"
  log "next: run ./secrets.sh if you haven't yet, then watch the boot:"
  log "  gcloud --project=$PROJECT compute ssh $INSTANCE_NAME --zone=$ZONE \\"
  log "    --command='sudo journalctl -u google-startup-scripts -f'"
  log "see ./README.md for the manual-once checklist (OAuth app, first login, webhook)."
}

main "$@"
