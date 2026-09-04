#!/bin/bash

set -euo pipefail

ROOT=$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)
ACCEPTANCE="$ROOT/test/marketplace-acceptance"

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

[[ -x $ACCEPTANCE && ! -L $ACCEPTANCE ]] || fail "marketplace acceptance runner exists"
text=$(<"$ACCEPTANCE")

for required in \
  'PLUGIN_ID=io.github.jaketothepast.adult-content-filter' \
  'REPOSITORY=https://github.com/jaketothepast/omarchy-adult-content-filter' \
  'omarchy plugin add "$REPOSITORY" --yes' \
  'omarchy plugin enable "$PLUGIN_ID" --section right' \
  'omarchy-plugin-validate "$PLUGIN_DIR"' \
  'omarchy-shell "$PLUGIN_ID" launch' \
  'omarchy-shell "$PLUGIN_ID" status' \
  'omarchy-shell "$PLUGIN_ID" stop' \
  'omarchy plugin disable "$PLUGIN_ID"' \
  'omarchy plugin remove "$PLUGIN_ID" --yes' \
  'run --images 17 --flagged-index 5 --hold-millis 1500 --assert-no-flash --json'; do
  [[ $text == *"$required"* ]] || fail "marketplace acceptance is missing: $required"
done

for forbidden in sudo pkexec useradd 'systemctl enable' '/usr/lib/omarchy-adult-content-filter'; do
  [[ $text != *"$forbidden"* ]] || fail "marketplace acceptance crosses the plugin boundary: $forbidden"
done

printf 'ok - marketplace acceptance exercises the cloned plugin and QML supervisor\n'
