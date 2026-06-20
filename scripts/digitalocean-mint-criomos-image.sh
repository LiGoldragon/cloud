#!/usr/bin/env bash
# Mint a re-usable PRE-MADE NixOS/CriomOS DigitalOcean image.
#
# DigitalOcean offers no NixOS distribution image and the account starts with
# zero custom images, so a "pre-made image" must be minted: provision a stock
# Ubuntu droplet, convert it to NixOS in place, snapshot it into a private
# image id, then tear the mint droplet down. The resulting numeric IMAGE_ID is
# what the deploy harness boots in MODE 1 (CRIOMOS_IMAGE=<id>).
#
# *** PRECONDITION — token scope ***
# The snapshot step (5) needs a DigitalOcean Personal Access Token carrying the
# image:create scope. The live token at `gopass digitalocean.com/api-token` is
# read + droplet/ssh-write only, so step 5 returns
#   403 "... missing the required permission image:create".
# Re-mint the PAT with image:create + image:read at the same gopass handle,
# then this script runs end to end. See reports/cloud-designer/73.
#
# Two conversion substrates (CONVERT env):
#   infect  (default) — nixos-infect: fast, GRUB-BIOS NixOS, minimal.
#   anywhere          — nixos-anywhere onto FLAKE (#FLAKE_ATTR): full CriomOS
#                       fidelity via disko; needs a >=2.5GB droplet and a flake
#                       whose nixosConfiguration boots GRUB on /dev/vda.
#
# DigitalOcean droplets boot legacy BIOS/GRUB only (UEFI unsupported), so the
# minted image is GRUB, and deploys onto it use nixos-rebuild switch, never
# lojix's systemd-boot BootOnce. See reports/cloud-designer/73 §2.
set -euo pipefail

API=https://api.digitalocean.com/v2
REGION=${DO_REGION:-nyc3}
SIZE=${DO_SIZE:-s-2vcpu-2gb}
IMAGE_NAME=${IMAGE_NAME:-criomos-base}
CONVERT=${CONVERT:-infect}
NIX_CHANNEL=${NIX_CHANNEL:-nixos-25.05}
FLAKE=${FLAKE:-}
FLAKE_ATTR=${FLAKE_ATTR:-target}

if [ -z "${DIGITALOCEAN_ACCESS_TOKEN:-}" ]; then
  DIGITALOCEAN_ACCESS_TOKEN=$(gopass show -o digitalocean.com/api-token)
fi
auth=(-H "Authorization: Bearer $DIGITALOCEAN_ACCESS_TOKEN" -H "Content-Type: application/json")

MINT_NAME="criomos-mint-$$"
DROPLET_ID=""
cleanup() {
  if [ -n "$DROPLET_ID" ]; then
    echo "cleanup: destroying mint droplet $DROPLET_ID" >&2
    curl -fsS "${auth[@]}" -X DELETE "$API/droplets/$DROPLET_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

echo "1. ensuring ssh key (uses ~/.ssh/id_ed25519.pub)"
PUBKEY=$(cat "${SSH_PUBKEY_FILE:-$HOME/.ssh/id_ed25519.pub}")
FINGERPRINT=$(curl -fsS "${auth[@]}" -X POST "$API/account/keys" \
  -d "{\"name\":\"$MINT_NAME\",\"public_key\":\"$PUBKEY\"}" \
  | jq -r '.ssh_key.fingerprint')

echo "2. provisioning stock ubuntu-24-04-x64 droplet in $REGION ($SIZE)"
DROPLET_ID=$(curl -fsS "${auth[@]}" -X POST "$API/droplets" \
  -d "{\"name\":\"$MINT_NAME\",\"region\":\"$REGION\",\"size\":\"$SIZE\",\"image\":\"ubuntu-24-04-x64\",\"ssh_keys\":[\"$FINGERPRINT\"],\"ipv6\":true,\"monitoring\":true}" \
  | jq -r '.droplet.id')
echo "   droplet id=$DROPLET_ID"

echo "3. polling to active + public IPv4"
IP=""
while [ -z "$IP" ]; do
  sleep 10
  IP=$(curl -fsS "${auth[@]}" "$API/droplets/$DROPLET_ID" \
    | jq -r '.droplet | select(.status=="active") | .networks.v4[] | select(.type=="public") | .ip_address' \
    | head -n1)
done
echo "   ipv4=$IP"

echo "4. converting in place to NixOS (CONVERT=$CONVERT)"
if [ "$CONVERT" = "anywhere" ]; then
  [ -n "$FLAKE" ] || { echo "CONVERT=anywhere needs FLAKE=<flake-ref>" >&2; exit 2; }
  nix run github:nix-community/nixos-anywhere -- \
    --flake "$FLAKE#$FLAKE_ATTR" --target-host "root@$IP"
else
  ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null "root@$IP" \
    "curl -fsSL https://raw.githubusercontent.com/elitak/nixos-infect/master/nixos-infect | NIX_CHANNEL=$NIX_CHANNEL PROVIDER=digitalocean bash -x" || true
  echo "   waiting for NixOS to come back up after reboot"
  until ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null \
    -o ConnectTimeout=5 "root@$IP" nixos-version >/dev/null 2>&1; do sleep 10; done
fi
ssh -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null "root@$IP" nixos-version

echo "5. snapshotting droplet -> reusable private image  (*** needs image:create scope ***)"
ACTION_ID=$(curl -fsS "${auth[@]}" -X POST "$API/droplets/$DROPLET_ID/actions" \
  -d "{\"type\":\"snapshot\",\"name\":\"$IMAGE_NAME-$(date +%Y%m%d 2>/dev/null || echo build)\"}" \
  | jq -r '.action.id')

echo "6. polling the snapshot action to completion (minutes)"
while [ "$(curl -fsS "${auth[@]}" "$API/droplets/$DROPLET_ID/actions/$ACTION_ID" | jq -r '.action.status')" != completed ]; do
  sleep 15
done

echo "7. resolving the minted numeric IMAGE_ID"
IMAGE_ID=$(curl -fsS "${auth[@]}" "$API/droplets/$DROPLET_ID/snapshots" | jq -r '.snapshots[0].id')

echo "MINTED IMAGE: id=$IMAGE_ID name=$IMAGE_NAME region=$REGION"
echo "Boot it with the deploy harness:"
echo "  CRIOMOS_IMAGE=$IMAGE_ID DO_REGION=$REGION nix run .#digitalocean-deploy-live-test"
# the EXIT trap destroys the mint droplet; the snapshot survives independently.
