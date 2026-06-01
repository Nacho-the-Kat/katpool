#!/usr/bin/env bash
# Network-aware deploy for the unified katpool runtime.
#
# One identical binary serves every network — the network is selected purely
# at runtime by the kaspad endpoint and the kaspatest:/kaspa: address prefix
# (see katpool/src/main.rs). This script therefore takes a *deploy-target*
# flag, not a build-time network: it builds (or accepts) the binary once and
# installs it, the tracked systemd unit, and the per-network env file into the
# correct per-network location, then restarts that network's service.
#
# Layout (symmetric; see docs/runbooks/09-deploy-and-rollback.md):
#   testnet : /root/katpool-tn10/katpool      katpool-tn10.service
#   mainnet : /root/katpool-mainnet/katpool   katpool-mainnet.service
#
# Usage (run as root):
#   scripts/deploy.sh --network <tn10|mainnet> [options]
#
# Options:
#   --network <tn10|mainnet>   target network (required)
#   --binary <path>            install this prebuilt binary instead of building
#                              (e.g. the signed musl release artifact)
#   --no-build                 reuse the existing dist binary; do not cargo build
#   --skip-restart             install everything but do not restart the service
#   -h, --help                 show this help
#
# Examples:
#   sudo scripts/deploy.sh --network tn10
#   sudo scripts/deploy.sh --network mainnet --binary /tmp/katpool

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
UNIT_TEMPLATE="${REPO_ROOT}/ops/systemd/katpool.service.in"
ETC_DIR=/etc/katpool
KEEP_BACKUPS=5

network=""
binary=""
do_build=1
do_restart=1

die() {
    echo "deploy: $*" >&2
    exit 1
}

usage() {
    sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --network)
            network="${2:-}"
            shift 2
            ;;
        --binary)
            binary="${2:-}"
            do_build=0
            shift 2
            ;;
        --no-build)
            do_build=0
            shift
            ;;
        --skip-restart)
            do_restart=0
            shift
            ;;
        -h | --help)
            usage 0
            ;;
        *)
            die "unknown argument: $1 (use --help)"
            ;;
    esac
done

case "${network}" in
    tn10 | mainnet) ;;
    "") die "missing required --network <tn10|mainnet>" ;;
    *) die "invalid --network '${network}' (expected tn10 or mainnet)" ;;
esac

if [[ ${EUID} -ne 0 ]]; then
    die "must be run as root (installs to /etc, /root, and restarts systemd)"
fi

deploy_dir="/root/katpool-${network}"
service="katpool-${network}"
env_src="${REPO_ROOT}/ops/env/${network}.env"
env_dst="${ETC_DIR}/${network}.env"
unit_dst="/etc/systemd/system/${service}.service"

[[ -f "${UNIT_TEMPLATE}" ]] || die "unit template not found: ${UNIT_TEMPLATE}"

if [[ ! -f "${env_src}" ]]; then
    die "missing ${env_src}: copy ops/env/${network}.env.example to ops/env/${network}.env and fill it in (the real env is host-local / gitignored)"
fi

# ----- Resolve the binary --------------------------------------------------
if [[ -n "${binary}" ]]; then
    [[ -f "${binary}" ]] || die "--binary path not found: ${binary}"
    src_bin="${binary}"
else
    target_dir="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}"
    src_bin="${target_dir}/dist/katpool"
    if [[ ${do_build} -eq 1 ]]; then
        echo "==> building dist binary (cargo build --profile dist --locked --bin katpool)"
        (cd "${REPO_ROOT}" && cargo build --profile dist --locked --bin katpool)
    fi
    [[ -f "${src_bin}" ]] || die "dist binary not found at ${src_bin} (drop --no-build, or pass --binary)"
fi

echo "==> deploying $(basename "${src_bin}") to ${deploy_dir} for service ${service}"

# ----- Install env + rendered unit -----------------------------------------
install -d -m 0750 "${ETC_DIR}"
install -m 0640 "${env_src}" "${env_dst}"
echo "    installed env  -> ${env_dst}"

tmp_unit="$(mktemp)"
trap 'rm -f "${tmp_unit}"' EXIT
sed -e "s|__NETWORK__|${network}|g" -e "s|__DEPLOY_DIR__|${deploy_dir}|g" \
    "${UNIT_TEMPLATE}" > "${tmp_unit}"
install -m 0644 "${tmp_unit}" "${unit_dst}"
echo "    installed unit -> ${unit_dst}"

# ----- Back up + install the binary ----------------------------------------
install -d -m 0755 "${deploy_dir}"
dst_bin="${deploy_dir}/katpool"
if [[ -f "${dst_bin}" ]]; then
    backup="${dst_bin}.bak-$(date -u +%Y%m%dT%H%M%SZ)"
    cp -p "${dst_bin}" "${backup}"
    echo "    backed up old  -> ${backup}"
    # Prune to the most recent ${KEEP_BACKUPS} backups.
    mapfile -t old_backups < <(ls -1t "${dst_bin}".bak-* 2>/dev/null | tail -n "+$((KEEP_BACKUPS + 1))")
    for f in "${old_backups[@]:-}"; do
        [[ -n "${f}" ]] && rm -f "${f}" && echo "    pruned backup  -> ${f}"
    done
fi
install -m 0755 "${src_bin}" "${dst_bin}"
echo "    installed bin  -> ${dst_bin}"

# ----- Activate ------------------------------------------------------------
systemctl daemon-reload
systemctl enable "${service}" >/dev/null 2>&1 || true

if [[ ${do_restart} -eq 0 ]]; then
    echo "==> --skip-restart: not restarting ${service}"
    echo "    start it with: systemctl restart ${service}"
    exit 0
fi

echo "==> restarting ${service}"
systemctl restart "${service}"
sleep 2

if systemctl is-active --quiet "${service}"; then
    echo "==> ${service} is active"
else
    echo "deploy: ${service} failed to start; last logs:" >&2
    journalctl -u "${service}" -n 40 --no-pager >&2 || true
    die "service not active after restart (binary backup retained for rollback)"
fi

echo "--- recent logs (${service}) ---"
journalctl -u "${service}" -n 15 --no-pager || true
echo
echo "Rollback: cp ${deploy_dir}/katpool.bak-<ts> ${dst_bin} && systemctl restart ${service}"
