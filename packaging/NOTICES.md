# Private Evaluation Notices

The resulting package and ISO are private and non-redistributable pending resolution of the Rust project's source license and the model weights' license and provenance.

## ONNX Runtime

The package includes ONNX Runtime 1.27.1 from the upstream `onnxruntime-linux-x64-1.27.1.tgz` archive pinned by SHA-256 `25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af`.

ONNX Runtime is distributed under the MIT License. Its upstream license and notices are installed with the runtime.

The ONNX Runtime license does not declare the Rust project or model weights to be MIT- or AGPL-licensed.

## Adult-domain policy

The package includes the `porn-only` hosts file from StevenBlack/hosts commit `2bb49d741a2c9b922b0ed59be6c28ce543bed81b`, pinned by SHA-256 `a512c2815fe612fa4eee8c7b1e2dab17f7a39016489eab3e3bbcc407edb4f514`. The matching `license.txt` is pinned by SHA-256 `8e52717212b5051232dd31fb2247310bd91d5295eaab380dbea6454d0268443b`.

The StevenBlack hosts repository is distributed under the MIT License. The list aggregates multiple sources; this package preserves the upstream license file and exact source identity without claiming perfect coverage or classification accuracy.

## NudeNet Model

The package includes `320n.onnx` from NudeNet commit `6ccc81c6c305cccfd46d92b414f8a5c0a816574d`, pinned by SHA-256 `c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f`.

At that commit, NudeNet's `LICENSE` is pinned by SHA-256 `8486a10c4393cee1c25392769ddd3b2d6c242d6ec7928e1414efff7dfb2f07ef`, while its `setup.py` is pinned by SHA-256 `acef07396af96db42374fc2f26799111d0989873f00da5494ac54a0ecf83ff09`. Those files contain conflicting AGPL and MIT license signals. They are installed as evidence, not as a declaration that the model weights use either license.

The model weights' separate license, training-data provenance, and redistribution terms remain unresolved.

## Rust Project

The Omarchy Kids Rust project has no selected source license. This notice grants no redistribution permission.
