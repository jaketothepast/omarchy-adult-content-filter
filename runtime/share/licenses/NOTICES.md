# Source and Third-Party Notices

## Omarchy Adult Content Filter

Project source and plugin code are distributed under the GNU Affero General Public License, version 3 only (`AGPL-3.0-only`). The complete terms are in the repository root `LICENSE` file and are included in the runtime bundle.

That license applies to this project's original code. The bundled third-party components retain the licenses and notices described below.

## ONNX Runtime

The bundle includes ONNX Runtime 1.27.1 from the upstream `onnxruntime-linux-x64-1.27.1.tgz` archive pinned by SHA-256 `25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af`.

ONNX Runtime is distributed under the MIT License. Its upstream license and third-party notices are included in the bundle.

## Adult-domain policy

The bundle includes the `porn-only` hosts file from StevenBlack/hosts commit `2bb49d741a2c9b922b0ed59be6c28ce543bed81b`, pinned by SHA-256 `a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514`. The matching `license.txt` is pinned by SHA-256 `8e52717212b5051232dd31fb2247310bd91d5295eaab380dbea6454d0268443b` and included in the bundle.

The StevenBlack hosts repository is distributed under the MIT License. The list aggregates multiple sources; this project preserves the upstream license and exact source identity without claiming perfect coverage or classification accuracy.

## NudeNet model

The bundle includes `320n.onnx` from NudeNet commit `6ccc81c6c305cccfd46d92b414f8a5c0a816574d`, pinned by SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.

At that commit, NudeNet's repository-root `LICENSE` is the GNU Affero General Public License version 3 and is pinned by SHA-256 `8486a10c4393cee1c25392769ddd3b2d6c242d6ec7928e1414efff7dfb2f07ef`. The model is redistributed with that license in the runtime bundle. The upstream `setup.py`, pinned by SHA-256 `acef07396af96db42374fc2f26799111d0989873f00da5494ac54a0ecf83ff09`, contains an older `MIT` package classifier; it is preserved alongside the license for transparency.

The model is a third-party classifier and comes without a promise of accuracy, fitness, or complete coverage. This project makes no representation about its training-data provenance.
