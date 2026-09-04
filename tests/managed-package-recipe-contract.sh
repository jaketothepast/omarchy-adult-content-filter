#!/bin/bash

set -euo pipefail

ROOT=$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)
recipe=${1:-$ROOT/packaging/arch/PKGBUILD}

fail() {
  printf 'not ok - %s\n' "$1" >&2
  exit 1
}

[[ -f $recipe && ! -L $recipe ]] || fail "managed-browser PKGBUILD exists"
/usr/bin/bash -n "$recipe" || fail "managed-browser PKGBUILD parses"

# shellcheck source=/dev/null
source "$recipe"

[[ $pkgname == omarchy-adult-content-filter ]] || fail "package identity is exact"
[[ $pkgver == 0.1.0 && $pkgrel == 1 ]] || fail "package version is exact"
[[ $url == https://github.com/jaketothepast/omarchy-adult-content-filter ]] || fail "package URL is public repository"
[[ ${license[*]} == AGPL-3.0-only ]] || fail "package source license is AGPL-3.0-only"
[[ ${depends[*]} == 'bash chromium coreutils gcc-libs glibc' ]] || fail "runtime dependencies are exact"
[[ ${makedepends[*]} == cargo ]] || fail "build dependencies are exact"
[[ ${options[*]} == '!debug' ]] || fail "debug split package is disabled"
[[ ${#source[@]} == 6 && ${#sha256sums[@]} == 6 ]] || fail "source and hash counts are exact"

expected_sources=(
  "onnxruntime-linux-x64-1.27.1.tgz::https://github.com/microsoft/onnxruntime/releases/download/v1.27.1/onnxruntime-linux-x64-1.27.1.tgz"
  '320n.onnx::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/nudenet/320n.onnx'
  'nudenet-LICENSE::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/LICENSE'
  'nudenet-setup.py::https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/setup.py'
  'adult-domains.hosts::https://raw.githubusercontent.com/StevenBlack/hosts/2bb49d741a2c9b922b0ed59be6c28ce543bed81b/alternates/porn-only/hosts'
  'stevenblack-license.txt::https://raw.githubusercontent.com/StevenBlack/hosts/2bb49d741a2c9b922b0ed59be6c28ce543bed81b/license.txt'
)
expected_hashes=(
  '25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af'
  'c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f'
  '8486a10c4393cee1c25392769ddd3b2d6c242d6ec7928e1414efff7dfb2f07ef'
  'acef07396af96db42374fc2f26799111d0989873f00da5494ac54a0ecf83ff09'
  'a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514'
  '8e52717212b5051232dd31fb2247310bd91d5295eaab380dbea6454d0268443b'
)
[[ ${source[*]} == "${expected_sources[*]}" ]] || fail "source URLs and names are exact"
[[ ${sha256sums[*]} == "${expected_hashes[*]}" ]] || fail "source hashes are exact"

recipe_text=$(<"$recipe")
for required in \
  'packaging/arch/omarchy-adult-content-filter' \
  'packaging/arch/omarchy-adult-content-filter.desktop' \
  'browser-extension/manifest.json' \
  'browser-extension/cover.css' \
  '$license_dir/LICENSE' \
  'policies/adult-domains.hosts' \
  '$license_dir/stevenblack-license.txt'; do
  [[ $recipe_text == *"$required"* ]] || fail "package installs $required"
done
for forbidden in '/etc/chromium' '.service' '/etc/systemd' '/etc/xdg/autostart' '/usr/share/mime'; do
  [[ $recipe_text != *"$forbidden"* ]] || fail "package excludes $forbidden"
done

printf 'ok - managed-browser package recipe contract\n'
