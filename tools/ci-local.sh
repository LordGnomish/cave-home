#!/usr/bin/env bash
# Run the same gates as .github/workflows/ci.yml, in the same order.
# Keep the two in sync: if you change a command here, change it there.
#
# Usage: tools/ci-local.sh [--quick]
#   --quick   skip the per-feature matrix and cargo-deny
#
# The kine-rs `grpc` and kubelet-rs `remote-cri` features need `protoc` on PATH.
set -euo pipefail
cd "$(dirname "$0")/.."

quick=0
[[ ${1:-} == "--quick" ]] && quick=1

step() { printf '\n==> %s\n' "$*"; "$@"; }

step cargo fmt --all -- --check
step cargo clippy --workspace --all-targets --locked -- -D warnings
step cargo test --workspace --all-targets --locked
step cargo test --workspace --doc --locked

if (( quick )); then
  printf '\nquick gates passed (feature matrix and cargo-deny skipped)\n'
  exit 0
fi

# package | features | test? (mirrors the `features` matrix in ci.yml)
matrix=(
  "cave-home-apiserver-rs|--features tls|test"
  "cave-home-binary|--features tls|test"
  "cave-home-mqtt|--features runtime|test"
  "cave-home-hue|--no-default-features|test"
  "cave-home-traefik-rs|--no-default-features|test"
  "cave-home-kine-rs|--features grpc|test"
  "cave-home-kine-rs|--features postgres,mysql|build-only"
  "cave-home-kubelet-rs|--features remote-cri|test"
)
for row in "${matrix[@]}"; do
  IFS='|' read -r pkg features mode <<<"$row"
  # shellcheck disable=SC2086 # $features is intentionally word-split
  step cargo clippy -p "$pkg" --all-targets --locked $features -- -D warnings
  if [[ $mode == test ]]; then
    # shellcheck disable=SC2086
    step cargo test -p "$pkg" --all-targets --locked $features
  fi
done

if command -v cargo-deny >/dev/null; then
  step cargo deny check
else
  printf '\ncargo-deny not installed (cargo install cargo-deny); skipping\n'
fi

printf '\nall CI gates passed\n'
