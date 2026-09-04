#!/bin/bash

set -euo pipefail

ROOT=$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)
BUILD_SCRIPT="$ROOT/scripts/build-plugin-bundle"
RECIPE="$ROOT/packaging/arch/PKGBUILD"
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
  "$@" >"$TEST_ROOT/stdout" 2>"$TEST_ROOT/stderr" || status=$?
  ((status == expected)) || fail "expected status $expected, got $status: $(<"$TEST_ROOT/stderr")"
}

make_package() {
  local fixture=$1
  local mutation=${2:-none}
  local tree="$fixture/package-root"
  local private_lib="$tree/usr/lib/omarchy-adult-content-filter"
  local private_share="$tree/usr/share/omarchy-adult-content-filter"
  local licenses="$tree/usr/share/licenses/omarchy-adult-content-filter"
  mkdir -p \
    "$tree/usr/bin" \
    "$tree/usr/share/applications" \
    "$private_lib/onnxruntime" \
    "$private_share/browser-extension" \
    "$private_share/models" \
    "$private_share/policies" \
    "$licenses"

  cat >"$tree/.PKGINFO" <<'EOF'
pkgname = omarchy-adult-content-filter
pkgver = 0.1.0-1
arch = x86_64
license = AGPL-3.0-only
EOF
  printf 'build\n' >"$tree/.BUILDINFO"
  printf 'mtree\n' >"$tree/.MTREE"
  printf '#!/bin/bash\n' >"$tree/usr/bin/omarchy-adult-content-filter"
  printf '[Desktop Entry]\n' >"$tree/usr/share/applications/omarchy-adult-content-filter.desktop"
  printf '#!/bin/bash\nprintf supervisor\\n\n' >"$private_lib/omarchy-adult-content-filter"
  printf 'onnx runtime\n' >"$private_lib/onnxruntime/libonnxruntime.so.1.27.1"
  ln -s libonnxruntime.so.1.27.1 "$private_lib/onnxruntime/libonnxruntime.so.1"
  ln -s libonnxruntime.so.1 "$private_lib/onnxruntime/libonnxruntime.so"
  printf '{}\n' >"$private_share/browser-extension/manifest.json"
  printf 'body {}\n' >"$private_share/browser-extension/cover.css"
  printf 'model\n' >"$private_share/models/320n.onnx"
  printf 'domains\n' >"$private_share/policies/adult-domains.hosts"
  for name in \
    LICENSE \
    NOTICES.md \
    nudenet-LICENSE \
    nudenet-setup.py \
    onnxruntime-LICENSE \
    onnxruntime-ThirdPartyNotices.txt \
    stevenblack-license.txt; do
    printf '%s\n' "$name" >"$licenses/$name"
  done
  chmod 0755 \
    "$tree/usr/bin/omarchy-adult-content-filter" \
    "$private_lib/omarchy-adult-content-filter" \
    "$private_lib/onnxruntime/libonnxruntime.so.1.27.1"

  case $mutation in
    wrong-name)
      sed -i 's/pkgname = omarchy-adult-content-filter/pkgname = another-package/' "$tree/.PKGINFO"
      ;;
    extra-path)
      printf 'unexpected\n' >"$private_share/unexpected"
      ;;
    escaped-link)
      ln -sfn ../../../../../../etc/passwd "$private_lib/onnxruntime/libonnxruntime.so.1"
      ;;
    none) ;;
    *) fail "unknown package fixture mutation: $mutation" ;;
  esac

  local package="$fixture/omarchy-adult-content-filter-0.1.0-1-x86_64.pkg.tar.zst"
  bsdtar -caf "$package" -C "$tree" .
  printf '%s\n' "$package"
}

make_plugin_fixture() {
  local name=$1
  local fixture="$TEST_ROOT/$name"
  mkdir -p "$fixture/plugin/scripts"
  cp -- "$BUILD_SCRIPT" "$fixture/plugin/scripts/build-plugin-bundle"
  chmod 0755 "$fixture/plugin/scripts/build-plugin-bundle"
  printf '%s\n' "$fixture"
}

[[ -f $RECIPE && ! -L $RECIPE ]] || fail "bundled package recipe exists in this repository"
[[ -x $BUILD_SCRIPT && ! -L $BUILD_SCRIPT ]] || fail "bundle builder exists in this repository"
[[ -f $ROOT/LICENSE && ! -L $ROOT/LICENSE ]] || fail "source license exists in this repository"
grep -Fq 'GNU AFFERO GENERAL PUBLIC LICENSE' "$ROOT/LICENSE" || fail "source license contains the AGPL-3.0 terms"
notices=$(<"$ROOT/packaging/NOTICES.md")
[[ $notices == *'Project source and plugin code'*AGPL-3.0* ]] || fail "notices identify the project source license"
[[ $notices != *'private and non-redistributable'* ]] || fail "notices no longer mark the public bundle private"
pass "repository owns its package recipe and bundle builder"

fixture=$(make_plugin_fixture success)
package=$(make_package "$fixture")
"$fixture/plugin/scripts/build-plugin-bundle" "$package"

runtime="$fixture/plugin/runtime"
expected_paths=$(cat <<'EOF'
SHA256SUMS
bin/omarchy-adult-content-filter
lib/onnxruntime/libonnxruntime.so
lib/onnxruntime/libonnxruntime.so.1
lib/onnxruntime/libonnxruntime.so.1.27.1
share/browser-extension/cover.css
share/browser-extension/manifest.json
share/licenses/LICENSE
share/licenses/NOTICES.md
share/licenses/nudenet-LICENSE
share/licenses/nudenet-setup.py
share/licenses/onnxruntime-LICENSE
share/licenses/onnxruntime-ThirdPartyNotices.txt
share/licenses/stevenblack-license.txt
share/models/320n.onnx
share/policies/adult-domains.hosts
EOF
)
actual_paths=$(find "$runtime" \( -type f -o -type l \) -printf '%P\n' | sort)
[[ $actual_paths == "$expected_paths" ]] || {
  diff -u <(printf '%s\n' "$expected_paths") <(printf '%s\n' "$actual_paths") >&2 || true
  fail "bundle contains the exact runtime allowlist"
}
[[ -x $runtime/bin/omarchy-adult-content-filter ]] || fail "supervisor remains executable"
[[ -x $runtime/lib/onnxruntime/libonnxruntime.so.1.27.1 ]] || fail "ONNX Runtime remains executable"
[[ $(readlink "$runtime/lib/onnxruntime/libonnxruntime.so.1") == libonnxruntime.so.1.27.1 ]] || fail "ONNX ABI link is exact"
[[ $(readlink "$runtime/lib/onnxruntime/libonnxruntime.so") == libonnxruntime.so.1 ]] || fail "ONNX linker link is exact"
(
  cd "$runtime"
  sha256sum --check --strict --quiet SHA256SUMS
) || fail "bundle checksum manifest verifies every regular payload"
pass "builder emits the exact verified runtime bundle"

for mutation in wrong-name extra-path escaped-link; do
  fixture=$(make_plugin_fixture "$mutation")
  mkdir -p "$fixture/plugin/runtime"
  printf 'preserve\n' >"$fixture/plugin/runtime/existing"
  package=$(make_package "$fixture" "$mutation")
  assert_status 78 "$fixture/plugin/scripts/build-plugin-bundle" "$package"
  [[ $(<"$fixture/plugin/runtime/existing") == preserve ]] || fail "$mutation failure preserves the prior runtime"
  [[ $(find "$fixture/plugin/runtime" -mindepth 1 -maxdepth 1 -printf '%f\n') == existing ]] || fail "$mutation failure leaves no staging residue in runtime"
done
pass "builder rejects wrong identity, extra paths, and symlink escapes atomically"

assert_status 64 "$BUILD_SCRIPT"
assert_status 64 "$BUILD_SCRIPT" one two
pass "builder accepts exactly one package argument"

recipe_text=$(<"$RECIPE")
[[ $recipe_text == *"url='https://github.com/jaketothepast/omarchy-adult-content-filter'"* ]] || fail "recipe identifies the public repository"
[[ $recipe_text == *"license=('AGPL-3.0-only')"* ]] || fail "recipe identifies the source license"
[[ $(grep -o '1\.27\.1' "$RECIPE" | wc -l) -eq 1 ]] || fail "recipe declares ONNX Runtime version once"
[[ $recipe_text != *'Private-Evaluation'* ]] || fail "recipe has no private-evaluation license"
pass "package recipe is public and keeps one runtime version source"
