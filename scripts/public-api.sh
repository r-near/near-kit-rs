#!/usr/bin/env bash
# Check or regenerate the committed public-API snapshots in `public-api/`.
#
#   scripts/public-api.sh           # check: fail with a diff if a snapshot is stale
#   scripts/public-api.sh --bless   # rewrite the snapshots after an intended change
#   scripts/public-api.sh --pins    # print the pinned versions (used by CI)
#
# Each snapshot is the `cargo public-api` listing of one crate under one
# feature set. Blanket and auto-trait impls are omitted: they churn with the
# nightly and the host, and release-plz's cargo-semver-checks run covers
# auto-trait (Send/Sync) regressions.
set -euo pipefail

# rustdoc JSON, and with it the snapshot text, changes between nightlies, so
# everyone (CI included) generates with exactly this pair. Bump both together,
# re-bless, and commit the snapshot churn on its own.
NIGHTLY=nightly-2026-09-17
CARGO_PUBLIC_API=0.52.0

# snapshot name | package | feature flags
SNAPSHOTS=(
  "near-kit|near-kit|--all-features"
  "near-kit-no-default-features|near-kit|--no-default-features"
  "near-kit-sandbox|near-kit-sandbox|--all-features"
)

BLESS_CMD="scripts/public-api.sh --bless"

root=$(cd "$(dirname "$0")/.." && pwd)
dir="$root/public-api"

mode=check
case "${1:-}" in
  "") ;;
  --bless) mode=bless ;;
  --pins)
    echo "nightly=$NIGHTLY"
    echo "cargo-public-api=$CARGO_PUBLIC_API"
    exit 0
    ;;
  *)
    echo "usage: $0 [--bless | --pins]" >&2
    exit 2
    ;;
esac

if [[ $(rustup toolchain list) != *"$NIGHTLY"* ]]; then
  rustup toolchain install "$NIGHTLY" --profile minimal
fi

installed=$(cargo "+$NIGHTLY" public-api --version 2>/dev/null || true)
if [[ "$installed" != "cargo-public-api $CARGO_PUBLIC_API" ]]; then
  echo "error: need cargo-public-api $CARGO_PUBLIC_API (found: ${installed:-none})" >&2
  echo "  cargo install --locked cargo-public-api@$CARGO_PUBLIC_API" >&2
  exit 1
fi

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

for entry in "${SNAPSHOTS[@]}"; do
  IFS='|' read -r name package features <<<"$entry"
  echo "==> $name (-p $package $features)" >&2
  # shellcheck disable=SC2086 # $features is a flag list
  (cd "$root" && cargo "+$NIGHTLY" public-api -p "$package" $features \
    --omit blanket-impls,auto-trait-impls --color never) >"$out/$name.txt"
done

if [[ $mode == bless ]]; then
  mkdir -p "$dir"
  rm -f "$dir"/*.txt
  cp "$out"/*.txt "$dir"/
  echo "Updated public-api/. Review and commit the diff." >&2
  exit 0
fi

stale=0
diffs=""
for entry in "${SNAPSHOTS[@]}"; do
  name=${entry%%|*}
  if d=$(cd "$root" && diff -u --label "a/public-api/$name.txt" \
    --label "b/public-api/$name.txt" "public-api/$name.txt" "$out/$name.txt" 2>&1); then
    continue
  fi
  stale=1
  diffs+="$d"$'\n'
done
# A snapshot that is committed but no longer generated is stale too.
for committed in "$dir"/*.txt; do
  [[ -e "$committed" && ! -e "$out/$(basename "$committed")" ]] || continue
  stale=1
  diffs+="unexpected snapshot: public-api/$(basename "$committed")"$'\n'
done

if [[ $stale == 0 ]]; then
  echo "Public API snapshots are up to date." >&2
  exit 0
fi

msg="The public API differs from the committed snapshot. If the change is
intended, run \`$BLESS_CMD\` and commit public-api/ with it."

printf '%s\n\n%s\n' "$diffs" "$msg" >&2
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "## Public API changed"
    echo
    echo "$msg"
    echo
    echo '```diff'
    # The summary is capped at 1 MiB; the full diff is in the job log.
    head -n 2000 <<<"$diffs"
    echo '```'
  } >>"$GITHUB_STEP_SUMMARY"
fi
exit 1
