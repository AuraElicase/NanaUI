#!/usr/bin/env bash
# Cross-check NanaUI crates for HarmonyOS / OpenHarmony ARM64
# (`aarch64-unknown-linux-ohos`). Check only: OHOS is a compile tier with no
# host in this repository (docs/reference/ohos.md), so there is no .so to build.
#
# Usage:
#   rustup target add aarch64-unknown-linux-ohos
#   ./scripts/check-ohos-arm64.sh
#
# Without a C toolchain this checks every crate that compiles no C. `fetch`
# (ureq → rustls → ring) runs a C build script even under `cargo check`, so the
# crates behind it are checked only when a target C compiler is available:
#   export OHOS_NDK_HOME=/path/to/ohos-sdk/linux   # contains native/llvm, native/sysroot
# or set CC_aarch64_unknown_linux_ohos yourself.
#
# Optional: export CARGO_TARGET_DIR=$PWD/target-ohos  (the default here)

set -eo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

TARGET=aarch64-unknown-linux-ohos
if [[ $# -gt 0 ]]; then
  echo "check-ohos-arm64: unknown argument $1 (check only; see docs/reference/ohos.md)" >&2
  exit 2
fi

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="${ROOT}/target-ohos"
fi
mkdir -p "${CARGO_TARGET_DIR}"

# The SDK root has `native/`; accept the `native` directory itself too.
if [[ -z "${CC_aarch64_unknown_linux_ohos:-}" && -n "${OHOS_NDK_HOME:-}" ]]; then
  NATIVE="${OHOS_NDK_HOME}"
  [[ -d "${NATIVE}/native" ]] && NATIVE="${NATIVE}/native"
  if [[ ! -x "${NATIVE}/llvm/bin/clang" || ! -d "${NATIVE}/sysroot" ]]; then
    echo "check-ohos-arm64: OHOS_NDK_HOME=${OHOS_NDK_HOME} has no native/llvm/bin/clang and native/sysroot" >&2
    exit 1
  fi
  export CC_aarch64_unknown_linux_ohos="${NATIVE}/llvm/bin/clang"
  export AR_aarch64_unknown_linux_ohos="${NATIVE}/llvm/bin/llvm-ar"
  export CFLAGS_aarch64_unknown_linux_ohos="--target=${TARGET} --sysroot=${NATIVE}/sysroot ${CFLAGS_aarch64_unknown_linux_ohos:-}"
fi

echo "check-ohos-arm64: target=${TARGET} CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"

run_crate() {
  local crate="$1"
  shift
  echo "---- cargo check -p ${crate} --target ${TARGET} $* ----"
  cargo check -p "${crate}" --target "${TARGET}" --locked "$@"
}

# No C build script anywhere below.
run_crate nana-diagnostics
run_crate nana-ui-core
run_crate nana-ui-input
run_crate nana-text
run_crate nana-text --tests
run_crate nana-ui-css
run_crate nana-ui-charts
run_crate nana-ui-diagram
run_crate nana-ui-runtime
run_crate nana-ui-scene
run_crate nana-gpu
run_crate nana-frame-exchange
run_crate nana-ui-platform --no-default-features --features clipboard
run_crate nana-ui-platform --no-default-features --features clipboard --tests
run_crate nana-ui --lib
run_crate nana-ui --lib --no-default-features --features components,bundled-fonts

if [[ -n "${CC_aarch64_unknown_linux_ohos:-}" ]]; then
  echo "check-ohos-arm64: C compiler ${CC_aarch64_unknown_linux_ohos}; checking the fetch / GPU path"
  run_crate nana-ui-platform
  run_crate nana-ui --lib --features gpu,components,bundled-fonts
else
  echo "check-ohos-arm64: no OHOS C compiler; fetch / GPU path not checked (set OHOS_NDK_HOME)"
fi

echo "check-ohos-arm64: OK"
