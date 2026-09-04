#!/bin/bash

set -euo pipefail

ROOT=$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)
WRAPPER="$ROOT/bin/omarchy-adult-content-filter"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf -- "$TEST_ROOT"' EXIT

pass() {
  printf 'ok - %s\n' "$1"
}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

assert_status() {
  local expected=$1
  shift
  local status=0
  "$@" >/dev/null 2>"$TEST_ROOT/stderr" || status=$?
  ((status == expected)) || fail "expected status $expected, got $status: $(<"$TEST_ROOT/stderr")"
}

make_fixture() {
  local name=$1
  local fixture="$TEST_ROOT/$name"
  local plugin="$fixture/plugin"
  local runtime="$plugin/runtime"

  mkdir -p \
    "$plugin/bin" \
    "$runtime/bin" \
    "$runtime/lib/onnxruntime" \
    "$runtime/share/models" \
    "$runtime/share/policies" \
    "$runtime/share/browser-extension" \
    "$fixture/xdg"
  chmod 0700 "$fixture/xdg"
  cp -- "$WRAPPER" "$plugin/bin/omarchy-adult-content-filter"
  chmod 0755 "$plugin/bin/omarchy-adult-content-filter"

  printf 'model\n' >"$runtime/share/models/320n.onnx"
  printf 'domains\n' >"$runtime/share/policies/adult-domains.hosts"
  printf '{}\n' >"$runtime/share/browser-extension/manifest.json"
  printf 'body {}\n' >"$runtime/share/browser-extension/cover.css"
  printf 'runtime\n' >"$runtime/lib/onnxruntime/libonnxruntime.so.1.27.1"
  chmod 0755 "$runtime/lib/onnxruntime/libonnxruntime.so.1.27.1"
  ln -s libonnxruntime.so.1.27.1 "$runtime/lib/onnxruntime/libonnxruntime.so.1"
  ln -s libonnxruntime.so.1 "$runtime/lib/onnxruntime/libonnxruntime.so"

  cat >"$runtime/bin/omarchy-adult-content-filter" <<'EOF'
#!/bin/bash
set -euo pipefail
{
  printf 'args=%q\n' "$*"
  printf 'HOME=%s\n' "$HOME"
  printf 'TMPDIR=%s\n' "$TMPDIR"
  printf 'PROFILE=%s\n' "$OMARCHY_KIDS_PROFILE_ROOT"
  printf 'CHROMIUM=%s\n' "$CHROMIUM_BIN"
  printf 'ORT=%s\n' "$ORT_DYLIB_PATH"
  printf 'MODEL=%s\n' "$NUDENET_MODEL_PATH"
  printf 'EXTENSION=%s\n' "$OMARCHY_KIDS_EXTENSION_DIR"
  printf 'BLOCKLIST=%s\n' "$OMARCHY_KIDS_BLOCKLIST_PATH"
} >"$PLUGIN_RUNTIME_TEST_RECORD"
touch "$PLUGIN_RUNTIME_TEST_STARTED"
if [[ -n ${PLUGIN_RUNTIME_TEST_RELEASE:-} ]]; then
  while [[ ! -e $PLUGIN_RUNTIME_TEST_RELEASE ]]; do sleep 0.01; done
fi
EOF
  chmod 0755 "$runtime/bin/omarchy-adult-content-filter"

  (
    cd "$runtime"
    sha256sum \
      bin/omarchy-adult-content-filter \
      lib/onnxruntime/libonnxruntime.so.1.27.1 \
      share/models/320n.onnx \
      share/policies/adult-domains.hosts \
      share/browser-extension/manifest.json \
      share/browser-extension/cover.css >SHA256SUMS
  )

  printf '%s\n' "$fixture"
}

[[ -f $WRAPPER ]] || fail "repository-local plugin wrapper exists"
grep -Fq '(( EUID != 0 ))' "$WRAPPER" || fail "wrapper refuses root"
if rg -n 'sudo|pkexec|systemctl|curl|wget' "$WRAPPER" >/dev/null; then
  fail "wrapper has no privilege, service, or remote-download surface"
fi
pass "wrapper exposes no privileged or remote installation path"

fixture=$(make_fixture success)
plugin="$fixture/plugin"
record="$fixture/record"
started="$fixture/started"
PLUGIN_RUNTIME_TEST_RECORD="$record" \
PLUGIN_RUNTIME_TEST_STARTED="$started" \
XDG_RUNTIME_DIR="$fixture/xdg" \
  "$plugin/bin/omarchy-adult-content-filter"

runtime_root="$fixture/xdg/omarchy-adult-content-filter"
grep -Fxq 'args=browse\ --json' "$record" || fail "wrapper executes only the managed browse command"
grep -Fxq "HOME=$runtime_root" "$record" || fail "wrapper confines HOME"
grep -Fxq "TMPDIR=$runtime_root" "$record" || fail "wrapper confines TMPDIR"
grep -Fxq "PROFILE=$runtime_root" "$record" || fail "wrapper confines the Chromium profile"
grep -Fxq 'CHROMIUM=/usr/bin/chromium' "$record" || fail "wrapper uses system Chromium"
grep -Fxq "ORT=$plugin/runtime/lib/onnxruntime/libonnxruntime.so.1" "$record" || fail "wrapper uses bundled ONNX Runtime"
grep -Fxq "MODEL=$plugin/runtime/share/models/320n.onnx" "$record" || fail "wrapper uses bundled model"
grep -Fxq "EXTENSION=$plugin/runtime/share/browser-extension" "$record" || fail "wrapper uses bundled extension"
grep -Fxq "BLOCKLIST=$plugin/runtime/share/policies/adult-domains.hosts" "$record" || fail "wrapper uses bundled domain policy"
pass "wrapper constructs the exact bundled supervisor environment"

assert_status 64 "$plugin/bin/omarchy-adult-content-filter" unexpected
pass "wrapper rejects public arguments before launch"

fixture=$(make_fixture bad-mode)
chmod 0755 "$fixture/xdg"
assert_status 78 env \
  PLUGIN_RUNTIME_TEST_RECORD="$fixture/record" \
  PLUGIN_RUNTIME_TEST_STARTED="$fixture/started" \
  XDG_RUNTIME_DIR="$fixture/xdg" \
  "$fixture/plugin/bin/omarchy-adult-content-filter"
[[ ! -e $fixture/started ]] || fail "unsafe runtime directory reached the supervisor"
pass "wrapper rejects an unsafe runtime directory before launch"

fixture=$(make_fixture corrupt)
printf 'corrupt\n' >>"$fixture/plugin/runtime/share/models/320n.onnx"
assert_status 78 env \
  PLUGIN_RUNTIME_TEST_RECORD="$fixture/record" \
  PLUGIN_RUNTIME_TEST_STARTED="$fixture/started" \
  XDG_RUNTIME_DIR="$fixture/xdg" \
  "$fixture/plugin/bin/omarchy-adult-content-filter"
[[ ! -e $fixture/started ]] || fail "corrupt bundle reached the supervisor"
pass "wrapper rejects corrupt bundled bytes before launch"

fixture=$(make_fixture escaped-link)
ln -sfn /etc/passwd "$fixture/plugin/runtime/lib/onnxruntime/libonnxruntime.so.1"
assert_status 78 env \
  PLUGIN_RUNTIME_TEST_RECORD="$fixture/record" \
  PLUGIN_RUNTIME_TEST_STARTED="$fixture/started" \
  XDG_RUNTIME_DIR="$fixture/xdg" \
  "$fixture/plugin/bin/omarchy-adult-content-filter"
[[ ! -e $fixture/started ]] || fail "escaped runtime link reached the supervisor"
pass "wrapper rejects a bundled symlink escape before launch"

fixture=$(make_fixture duplicate)
release="$fixture/release"
PLUGIN_RUNTIME_TEST_RECORD="$fixture/first-record" \
PLUGIN_RUNTIME_TEST_STARTED="$fixture/first-started" \
PLUGIN_RUNTIME_TEST_RELEASE="$release" \
XDG_RUNTIME_DIR="$fixture/xdg" \
  "$fixture/plugin/bin/omarchy-adult-content-filter" &
first_pid=$!
for _ in $(seq 1 200); do
  [[ -e $fixture/first-started ]] && break
  sleep 0.01
done
[[ -e $fixture/first-started ]] || fail "first supervisor starts"
assert_status 75 env \
  PLUGIN_RUNTIME_TEST_RECORD="$fixture/second-record" \
  PLUGIN_RUNTIME_TEST_STARTED="$fixture/second-started" \
  XDG_RUNTIME_DIR="$fixture/xdg" \
  "$fixture/plugin/bin/omarchy-adult-content-filter"
[[ ! -e $fixture/second-started ]] || fail "duplicate launch reached a second supervisor"
touch "$release"
wait "$first_pid"
pass "wrapper permits exactly one supervisor per user"
