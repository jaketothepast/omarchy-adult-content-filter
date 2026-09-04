# Source and Third-Party Notices

## Omarchy Adult Content Filter

Original project and plugin code are distributed under the GNU Affero General Public License, version 3 only (`AGPL-3.0-only`). The complete terms are in [LICENSE](LICENSE) and are included in the runtime bundle.

That license applies to this project's original code. Bundled third-party components retain the licenses and notices described below.

## ONNX Runtime

The bundle includes ONNX Runtime 1.27.1 from the upstream `onnxruntime-linux-x64-1.27.1.tgz` archive, pinned by SHA-256 `25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af`.

ONNX Runtime is distributed under the MIT License. Its upstream license and third-party notices are bundled at `runtime/share/licenses/onnxruntime-LICENSE` and `runtime/share/licenses/onnxruntime-ThirdPartyNotices.txt`.

## Adult-domain policy

The bundle includes the `porn-only` hosts file from StevenBlack/hosts commit `2bb49d741a2c9b922b0ed59be6c28ce543bed81b`, pinned by SHA-256 `a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514`. Its matching license is pinned by SHA-256 `8e52717212b5051232dd31fb2247310bd91d5295eaab380dbea6454d0268443b` and bundled at `runtime/share/licenses/stevenblack-license.txt`.

The StevenBlack hosts repository is distributed under the MIT License. The list aggregates multiple sources; this project preserves the upstream license and exact source identity without claiming perfect coverage or classification accuracy.

## NudeNet model

The bundle includes NudeNet `320n.onnx` from commit `6ccc81c6c305cccfd46d92b414f8a5c0a816574d`, pinned by SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.

At that commit, the NudeNet repository-root license is GNU Affero General Public License version 3 and is pinned by SHA-256 `8486a10c4393cee1c25392769ddd3b2d6c242d6ec7928e1414efff7dfb2f07ef`. It is bundled at `runtime/share/licenses/nudenet-LICENSE`. The upstream `setup.py`, pinned by SHA-256 `acef07396af96db42374fc2f26799111d0989873f00da5494ac54a0ecf83ff09`, contains an older `MIT` package classifier; those exact source bytes are preserved as non-executable `runtime/share/licenses/nudenet-package-metadata.txt` alongside the license for transparency.

The model is a third-party classifier and comes without a promise of accuracy, fitness, complete coverage, or training-data provenance.
