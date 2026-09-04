#!/bin/bash

set -euo pipefail

PACKAGE=omarchy-kids-browser-filter-demo
INSTALL_ROOT=${OMARCHY_KIDS_INSTALL_ROOT:-}
ARTIFACTS=${OMARCHY_ACCEPTANCE_DIR:-/tmp/omarchy-external-acceptance}
PROFILE_ROOT=${OMARCHY_KIDS_PROFILE_ROOT:-${TMPDIR:-/tmp}}

mkdir -p "$ARTIFACTS"

pass() {
  printf 'ok - %s\n' "$1"
}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

if pacman -Q "$PACKAGE" >/dev/null 2>&1; then
  pass "$PACKAGE package is installed"
else
  fail "$PACKAGE package is installed"
fi

installed_path() {
  printf '%s%s\n' "$INSTALL_ROOT" "$1"
}

required_files=(
  /usr/bin/omarchy-kids-browser-filter-demo
  /usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter
  /usr/share/omarchy-kids-browser-filter-demo/browser-extension/manifest.json
  /usr/share/omarchy-kids-browser-filter-demo/browser-extension/cover.css
  /usr/share/applications/omarchy-kids-browser-filter-demo.desktop
  /usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx
  /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1
  /usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-LICENSE
  /usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-ThirdPartyNotices.txt
  /usr/share/licenses/omarchy-kids-browser-filter-demo/NOTICES.md
  /usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-LICENSE
  /usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-setup.py
)

allowed_non_directory_paths=(
  "${required_files[@]}"
  /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1
  /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so
)

for path in "${required_files[@]}"; do
  [[ -f $(installed_path "$path") && ! -L $(installed_path "$path") ]] || fail "required installed path exists: $path"
done

assert_metadata() {
  local path=$1 mode=$2
  local metadata owner

  owner=$(pacman -Qqo "$(installed_path "$path")") || fail "required path is owned by $PACKAGE: $path"
  [[ $owner == "$PACKAGE" ]] || fail "required path is owned by $PACKAGE: $path"
  metadata=$(stat -c '%U:%G %a' "$(installed_path "$path")") || fail "required installed ownership and mode: $path"
  [[ $metadata == "root:root $mode" ]] || fail "required installed ownership and mode: $path"
}

for path in "${required_files[@]}"; do
  case $path in
    /usr/bin/omarchy-kids-browser-filter-demo|/usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter|/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1)
      assert_metadata "$path" 755
      ;;
    *)
      assert_metadata "$path" 644
      ;;
  esac
done

extension_dir=$(installed_path /usr/share/omarchy-kids-browser-filter-demo/browser-extension)
[[ -d $extension_dir ]] || fail "required installed path exists: /usr/share/omarchy-kids-browser-filter-demo/browser-extension"
assert_metadata /usr/share/omarchy-kids-browser-filter-demo/browser-extension 755
extension_entries=$(find "$extension_dir" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort)
[[ $extension_entries == $'cover.css\nmanifest.json' ]] || fail "browser extension contains exactly manifest.json and cover.css"

runtime_link=$(installed_path /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1)
[[ -L $runtime_link ]] || fail "required installed path exists: /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1"
[[ $(readlink "$runtime_link") == "libonnxruntime.so.1.27.1" ]] || fail "ONNX Runtime major-version symlink is exact"
assert_metadata /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1 777

runtime_link=$(installed_path /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so)
[[ -L $runtime_link ]] || fail "required installed path exists: /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so"
[[ $(readlink "$runtime_link") == "libonnxruntime.so.1" ]] || fail "ONNX Runtime unversioned symlink is exact"
assert_metadata /usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so 777

package_listing=$(pacman -Qlq "$PACKAGE") || fail "package file list is available"
mapfile -t package_paths <<< "$package_listing"
for path in "${package_paths[@]}"; do
  [[ -n $path ]] || continue
  rooted_path=$(installed_path "$path")
  if [[ -d $rooted_path && ! -L $rooted_path ]]; then
    continue
  fi

  allowed=false
  for allowed_path in "${allowed_non_directory_paths[@]}"; do
    if [[ $path == "$allowed_path" ]]; then
      allowed=true
      break
    fi
  done
  if ! $allowed; then
    fail "package contains only expected non-directory paths: $path"
  fi
done
pass "package contains only expected non-directory paths"

pass "required installed paths and extension contents are complete"

model_hash=$(sha256sum "$(installed_path /usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx)")
model_hash=${model_hash%% *}
[[ $model_hash == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f" ]] || fail "installed model SHA-256 matches the pinned model"
pass "installed model SHA-256 matches the pinned model"

baseline_clients=$(hyprctl -j clients) || fail "headed Chromium client is observed for this launch"
baseline_addresses=$(jq -c '[.[] | select((.class // "") | test("(?i)chromium")) | .address]' <<<"$baseline_clients") || fail "headed Chromium client is observed for this launch"
baseline_pids=$(pgrep -f -- 'chromium.*omarchy-kids-browser-' || true)

summary="$ARTIFACTS/browser-filter-summary.json"
metrics="$ARTIFACTS/browser-filter-metrics.jsonl"

omarchy-kids-browser-filter-demo \
  --images 17 \
  --flagged-index 5 \
  --hold-millis 1500 \
  --assert-no-flash \
  --json \
  >"$summary" 2>"$metrics" &
launcher_pid=$!

headed_client_observed=false
client_deadline=$((SECONDS + ${OMARCHY_KIDS_CLIENT_TIMEOUT:-60}))
while kill -0 "$launcher_pid" 2>/dev/null; do
  current_clients=$(hyprctl -j clients 2>/dev/null || printf '[]\n')
  if jq -e --argjson baseline "$baseline_addresses" '([.[] | select((.class // "") | test("(?i)chromium")) | .address] - $baseline) | length > 0' <<<"$current_clients" >/dev/null; then
    headed_client_observed=true
    break
  fi

  if ((SECONDS >= client_deadline)); then
    break
  fi
  sleep "${OMARCHY_KIDS_CLIENT_POLL_SECONDS:-0.25}"
done

launcher_status=0
wait "$launcher_pid" || launcher_status=$?

[[ $headed_client_observed == true ]] || fail "headed Chromium client is observed for this launch"
((launcher_status == 0)) || fail "controlled demo launcher exits successfully"
pass "headed Chromium client was observed for this launch"

[[ -s $summary ]] || fail "browser summary artifact is nonempty"
jq -e '
  .intercepted == 17 and
  .continued == 16 and
  .replaced == 1 and
  .unresolved == 0 and
  .clean_shutdown == true and
  .onnx_runtime_version == "1.27.1" and
  .model_sha256 == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f" and
  (.reveal_latency_millis | type == "number" and . >= 0 and . <= 500) and
  .no_flash_assertion.requested_hold_millis == 1500 and
  (.no_flash_assertion.actual_hold_millis | type == "number" and . >= 1500) and
  (.no_flash_assertion.hold_screenshot_count | type == "number" and . >= 3) and
  .no_flash_assertion.hold_sampled_pixels > 0 and
  .no_flash_assertion.cover_rgba == [17,19,24,255] and
  .no_flash_assertion.safe_fixture_colors_present == 16 and
  .no_flash_assertion.placeholder_color_present == true and
  .no_flash_assertion.original_flagged_color_absent == true and
  (.dom_images | length) == 17 and
  .dom_images[5].rgba == [255,0,255,255]
' "$summary" >/dev/null 2>&1 || fail "browser summary proves the controlled fixture contract"
pass "browser summary proves the controlled fixture contract"

[[ -s $metrics ]] || fail "browser metrics artifact is nonempty"
metric_count=$(wc -l <"$metrics")
((metric_count == 34)) || fail "browser metrics contain exactly 34 records"
jq -se 'length == 34 and all(.[]; type == "object" and keys == ["elapsed_micros", "fixture_index", "stage", "verdict"])' "$metrics" >/dev/null 2>&1 || fail "browser metrics contain only privacy-safe keys"
pass "browser metrics contain exactly 34 privacy-safe records"

[[ -s $ARTIFACTS/acceptance.log ]] || fail "external acceptance log is nonempty"

leaked_profile=$(find "$PROFILE_ROOT" -maxdepth 1 -type d -name 'omarchy-kids-browser-*' -print -quit)
[[ -z $leaked_profile ]] || fail "disposable Chromium profile is removed"

current_pids=$(pgrep -f -- 'chromium.*omarchy-kids-browser-' || true)
while IFS= read -r pid; do
  [[ -n $pid ]] || continue
  if ! grep -Fx -- "$pid" <<<"$baseline_pids" >/dev/null; then
    fail "launched Chromium process is gone"
  fi
done <<<"$current_pids"

pass "controlled demo artifacts and cleanup are complete"
