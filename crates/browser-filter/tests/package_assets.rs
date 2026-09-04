use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn workspace_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn parse_desktop_entry(contents: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut current_section = None;

    for (line_number, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            assert!(
                !section.is_empty(),
                "empty section at line {}",
                line_number + 1
            );
            assert!(
                sections
                    .insert(section.to_owned(), BTreeMap::new())
                    .is_none(),
                "duplicate section {section}"
            );
            current_section = Some(section.to_owned());
            continue;
        }

        let (key, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("invalid desktop entry line {}", line_number + 1));
        assert!(!key.is_empty(), "empty key at line {}", line_number + 1);
        let section = current_section
            .as_ref()
            .unwrap_or_else(|| panic!("key before section at line {}", line_number + 1));
        assert!(
            sections
                .get_mut(section)
                .unwrap()
                .insert(key.to_owned(), value.to_owned())
                .is_none(),
            "duplicate key {key} in section {section}"
        );
    }

    sections
}

// Production mutation caught: changing an installed path, dropping strict shell mode, or failing
// to forward launcher arguments would make the dedicated entry point run a different boundary.
#[test]
fn wrapper_exports_exact_installed_paths_and_execs_private_run_command() {
    let wrapper = workspace_path("packaging/arch/omarchy-kids-browser-filter-demo");
    let syntax = Command::new("bash")
        .arg("-n")
        .arg(&wrapper)
        .output()
        .expect("failed to run bash syntax parser");
    assert!(
        syntax.status.success(),
        "wrapper syntax failed: {}",
        String::from_utf8_lossy(&syntax.stderr)
    );

    let contents = fs::read_to_string(&wrapper).expect("wrapper asset is missing");
    let lines = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines[0], "#!/bin/bash");
    assert_eq!(lines[1], "set -euo pipefail");

    let mut exports = BTreeMap::new();
    for line in &lines[2..6] {
        let assignment = line
            .strip_prefix("export ")
            .unwrap_or_else(|| panic!("expected export statement, got {line:?}"));
        let (name, value) = assignment
            .split_once('=')
            .unwrap_or_else(|| panic!("invalid export assignment {assignment:?}"));
        assert!(
            exports.insert(name, value).is_none(),
            "duplicate export {name}"
        );
    }
    assert_eq!(
        exports,
        BTreeMap::from([
            ("CHROMIUM_BIN", "/usr/bin/chromium"),
            (
                "NUDENET_MODEL_PATH",
                "/usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx",
            ),
            (
                "OMARCHY_KIDS_EXTENSION_DIR",
                "/usr/share/omarchy-kids-browser-filter-demo/browser-extension",
            ),
            (
                "ORT_DYLIB_PATH",
                "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1",
            ),
        ])
    );

    let exec = lines[6]
        .strip_prefix("exec ")
        .expect("final wrapper statement must be exec");
    assert_eq!(
        exec.split_ascii_whitespace().collect::<Vec<_>>(),
        [
            "/usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter",
            "run",
            "\"$@\"",
        ]
    );
    assert_eq!(lines.len(), 7, "wrapper contains behavior after the exec");
}

// Production mutation caught: changing the fixture arguments or declaring browser MIME handling
// would turn the controlled demo launcher into a different experiment or browser entry point.
#[test]
fn desktop_entry_runs_only_the_controlled_fixture() {
    let desktop = fs::read_to_string(workspace_path(
        "packaging/arch/omarchy-kids-browser-filter-demo.desktop",
    ))
    .expect("desktop asset is missing");
    let sections = parse_desktop_entry(&desktop);

    assert_eq!(sections.keys().collect::<Vec<_>>(), ["Desktop Entry"]);
    let entry = &sections["Desktop Entry"];
    assert_eq!(entry["Type"], "Application");
    assert_eq!(entry["Name"], "Managed Browser Filter — Controlled Demo");
    assert_eq!(
        entry["Exec"].split_ascii_whitespace().collect::<Vec<_>>(),
        [
            "omarchy-kids-browser-filter-demo",
            "--images",
            "17",
            "--flagged-index",
            "5",
            "--hold-millis",
            "500",
            "--assert-no-flash",
        ]
    );
    assert_eq!(entry["Terminal"], "false");
    assert!(entry.get("MimeType").is_none());
}

// Production mutation caught: omitting either pinned identity or relaxing the unresolved license
// warning could allow the private evaluation package or ISO to be treated as redistributable.
#[test]
fn notices_pin_runtime_and_model_and_forbid_redistribution() {
    let notices = fs::read_to_string(workspace_path("packaging/NOTICES.md"))
        .expect("private-evaluation notice is missing");

    assert!(notices.contains("25b1ef1fea1acd210d63f8f24dc870ad6e077795ce1f54876252c6d3803c15af"));
    assert!(notices.contains("c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f"));
    assert!(notices.contains("ONNX Runtime is distributed under the MIT License."));
    assert!(notices.contains(
        "The resulting package and ISO are private and non-redistributable pending resolution of the Rust project's source license and the model weights' license and provenance."
    ));
    assert!(notices.contains(
        "The ONNX Runtime license does not declare the Rust project or model weights to be MIT- or AGPL-licensed."
    ));
}
