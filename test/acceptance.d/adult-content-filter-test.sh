#!/bin/bash

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../lib/adult-content-filter-process.sh
source "$SCRIPT_DIR/../lib/adult-content-filter-process.sh"

PACKAGE=omarchy-adult-content-filter
INSTALL_ROOT=${OMARCHY_ADULT_FILTER_INSTALL_ROOT:-}
ARTIFACTS=${OMARCHY_ACCEPTANCE_DIR:-/tmp/omarchy-external-acceptance}
PROFILE_ROOT=${OMARCHY_ADULT_FILTER_PROFILE_ROOT:-${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR is not set}/omarchy-adult-content-filter}

mkdir -p "$ARTIFACTS" "$PROFILE_ROOT"

pass() {
  printf 'ok - %s\n' "$1"
}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

installed_path() {
  printf '%s%s\n' "$INSTALL_ROOT" "$1"
}

if pacman -Q "$PACKAGE" >/dev/null 2>&1; then
  pass "$PACKAGE package is installed"
else
  fail "$PACKAGE package is installed"
fi

required_files=(
  /usr/bin/omarchy-adult-content-filter
  /usr/lib/omarchy-adult-content-filter/omarchy-adult-content-filter
  /usr/share/omarchy-adult-content-filter/browser-extension/manifest.json
  /usr/share/omarchy-adult-content-filter/browser-extension/cover.css
  /usr/share/applications/omarchy-adult-content-filter.desktop
  /usr/share/omarchy-adult-content-filter/models/320n.onnx
  /usr/share/omarchy-adult-content-filter/policies/adult-domains.hosts
  /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1.27.1
  /usr/share/licenses/omarchy-adult-content-filter/onnxruntime-LICENSE
  /usr/share/licenses/omarchy-adult-content-filter/onnxruntime-ThirdPartyNotices.txt
  /usr/share/licenses/omarchy-adult-content-filter/NOTICES.md
  /usr/share/licenses/omarchy-adult-content-filter/nudenet-LICENSE
  /usr/share/licenses/omarchy-adult-content-filter/nudenet-setup.py
  /usr/share/licenses/omarchy-adult-content-filter/stevenblack-license.txt
)

allowed_non_directory_paths=(
  "${required_files[@]}"
  /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1
  /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so
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
    /usr/bin/omarchy-adult-content-filter|/usr/lib/omarchy-adult-content-filter/omarchy-adult-content-filter|/usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1.27.1)
      assert_metadata "$path" 755
      ;;
    *)
      assert_metadata "$path" 644
      ;;
  esac
done

extension_dir=$(installed_path /usr/share/omarchy-adult-content-filter/browser-extension)
[[ -d $extension_dir ]] || fail "required installed path exists: /usr/share/omarchy-adult-content-filter/browser-extension"
assert_metadata /usr/share/omarchy-adult-content-filter/browser-extension 755
extension_entries=$(find "$extension_dir" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort)
[[ $extension_entries == $'cover.css\nmanifest.json' ]] || fail "browser extension contains exactly manifest.json and cover.css"

runtime_link=$(installed_path /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1)
[[ -L $runtime_link && $(readlink "$runtime_link") == "libonnxruntime.so.1.27.1" ]] || fail "ONNX Runtime major-version symlink is exact"
assert_metadata /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1 777
runtime_link=$(installed_path /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so)
[[ -L $runtime_link && $(readlink "$runtime_link") == "libonnxruntime.so.1" ]] || fail "ONNX Runtime unversioned symlink is exact"
assert_metadata /usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so 777

package_listing=$(pacman -Qlq "$PACKAGE") || fail "package file list is available"
mapfile -t package_paths <<<"$package_listing"
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
  $allowed || fail "package contains only expected non-directory paths: $path"
done
pass "package contains only expected non-directory paths"

model_hash=$(sha256sum "$(installed_path /usr/share/omarchy-adult-content-filter/models/320n.onnx)")
model_hash=${model_hash%% *}
[[ $model_hash == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f" ]] || fail "installed model SHA-256 matches the pinned model"
policy_hash=$(sha256sum "$(installed_path /usr/share/omarchy-adult-content-filter/policies/adult-domains.hosts)")
policy_hash=${policy_hash%% *}
[[ $policy_hash == "a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514" ]] || fail "installed adult-domain policy matches the pinned policy"
pass "installed model and adult-domain policy identities are pinned"

baseline_clients=$(hyprctl -j clients) || fail "managed browser launcher exits successfully"
managed_summary="$ARTIFACTS/managed-browser-summary.json"
managed_metrics="$ARTIFACTS/managed-browser-metrics.jsonl"

omarchy-adult-content-filter >"$managed_summary" 2>"$managed_metrics" &
managed_pid=$!
managed_address=
browser_pid=
client_deadline=$((SECONDS + ${OMARCHY_ADULT_FILTER_CLIENT_TIMEOUT:-60}))
while kill -0 "$managed_pid" 2>/dev/null; do
  current_clients=$(hyprctl -j clients 2>/dev/null || printf '[]\n')
  managed_client=$(adult_filter_new_chromium_client "$baseline_clients" "$current_clients")
  if [[ -n $managed_client ]]; then
    IFS=$'\t' read -r managed_address browser_pid <<<"$managed_client"
  fi
  if [[ -n $managed_address && -n $browser_pid ]]; then
    break
  fi
  ((SECONDS < client_deadline)) || break
  sleep "${OMARCHY_ADULT_FILTER_CLIENT_POLL_SECONDS:-0.25}"
done

[[ -n $managed_address && -n $browser_pid ]] || {
  kill "$managed_pid" 2>/dev/null || true
  wait "$managed_pid" 2>/dev/null || true
  fail "managed browser window and PID are observed"
}
browser_cmdline=/proc/$browser_pid/cmdline
adult_filter_cmdline_has_exact_argument "$browser_cmdline" --disable-dev-tools || fail "managed browser disables developer tools"
adult_filter_cmdline_has_exact_argument "$browser_cmdline" --ozone-platform=wayland || fail "managed browser uses native Wayland"
adult_filter_cmdline_has_argument_prefix \
  "$browser_cmdline" \
  --user-data-dir=/run/user/1000/omarchy-adult-content-filter/omarchy-kids-browser- || fail "managed browser uses its private disposable profile"

if ! close_result=$(adult_filter_close_window "$managed_address" 2>&1); then
  printf 'managed browser close failed for %s: %s\n' "$managed_address" "$close_result" >&2
  fail "managed browser window accepts a close request"
fi
managed_status=0
wait "$managed_pid" || managed_status=$?
if ((managed_status != 0)); then
  printf 'managed browser launcher exit status: %d\n' "$managed_status" >&2
  fail "managed browser launcher exits successfully"
fi
jq -e '
  .blocklist_entries == 76767 and
  .domain_blocked_requests == 0 and
  .safe_search_rewrites == 0 and
  .youtube_restricted_requests == 0 and
  .intercepted == 0 and
  .unresolved == 0 and
  .clean_shutdown == true and
  .onnx_runtime_version == "1.27.1" and
  .model_sha256 == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f"
' "$managed_summary" >/dev/null 2>&1 || fail "managed browser launcher emits its clean private-browser summary"
pass "managed browser launcher exits successfully"

summary="$ARTIFACTS/browser-filter-summary.json"
metrics="$ARTIFACTS/browser-filter-metrics.jsonl"
CHROMIUM_BIN=/usr/bin/chromium \
ORT_DYLIB_PATH=/usr/lib/omarchy-adult-content-filter/onnxruntime/libonnxruntime.so.1 \
NUDENET_MODEL_PATH=/usr/share/omarchy-adult-content-filter/models/320n.onnx \
OMARCHY_KIDS_EXTENSION_DIR=/usr/share/omarchy-adult-content-filter/browser-extension \
OMARCHY_KIDS_BLOCKLIST_PATH=/usr/share/omarchy-adult-content-filter/policies/adult-domains.hosts \
  /usr/lib/omarchy-adult-content-filter/omarchy-adult-content-filter run \
    --images 17 --flagged-index 5 --hold-millis 1500 --assert-no-flash --json \
    >"$summary" 2>"$metrics" || fail "controlled detector proof exits successfully"

jq -e '
  .intercepted == 17 and .continued == 16 and .replaced == 1 and
  .unresolved == 0 and .clean_shutdown == true and
  .onnx_runtime_version == "1.27.1" and
  .model_sha256 == "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f" and
  (.reveal_latency_millis | type == "number" and . >= 0 and . <= 500) and
  .no_flash_assertion.requested_hold_millis == 1500 and
  (.no_flash_assertion.actual_hold_millis | type == "number" and . >= 1500) and
  (.no_flash_assertion.hold_screenshot_count | type == "number" and . >= 3) and
  .no_flash_assertion.cover_rgba == [17,19,24,255] and
  .no_flash_assertion.safe_fixture_colors_present == 16 and
  .no_flash_assertion.placeholder_color_present == true and
  .no_flash_assertion.original_flagged_color_absent == true and
  (.dom_images | length) == 17 and .dom_images[5].rgba == [255,0,255,255]
' "$summary" >/dev/null 2>&1 || fail "controlled detector proof exits successfully"
[[ $(wc -l <"$metrics") -eq 34 ]] || fail "controlled detector proof exits successfully"
jq -se 'length == 34 and all(.[]; type == "object" and keys == ["elapsed_micros", "fixture_index", "stage", "verdict"])' "$metrics" >/dev/null 2>&1 || fail "controlled detector proof exits successfully"
pass "controlled detector proof exits successfully"

[[ -s $ARTIFACTS/acceptance.log ]] || fail "external acceptance log is nonempty"
leaked_profile=$(find "$PROFILE_ROOT" -maxdepth 1 -type d -name 'omarchy-kids-browser-*' -print -quit)
[[ -z $leaked_profile ]] || fail "disposable Chromium profile is removed"
current_pids=$(pgrep -f -- 'chromium.*omarchy-adult-content-filter' || true)
while IFS= read -r pid; do
  [[ -n $pid ]] || continue
  grep -Fx -- "$pid" <<<"$baseline_pids" >/dev/null || fail "launched Chromium process is gone"
done <<<"$current_pids"
pass "browser package artifacts and cleanup are complete"
