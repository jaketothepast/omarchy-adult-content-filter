use std::{fs, path::PathBuf};

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn read(relative: &str) -> String {
    fs::read_to_string(workspace_path(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

#[test]
fn manifest_declares_one_kept_service_and_widget() {
    let manifest: serde_json::Value =
        serde_json::from_str(&read("manifest.json")).expect("manifest must be valid JSON");

    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(
        manifest["id"],
        "io.github.jaketothepast.adult-content-filter"
    );
    assert_eq!(manifest["name"], "Omarchy Adult Content Filter");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["license"], "AGPL-3.0-only");
    assert_eq!(
        manifest["kinds"],
        serde_json::json!(["service", "bar-widget"])
    );
    assert_eq!(manifest["keepLoaded"], true);
    assert_eq!(manifest["entryPoints"]["service"], "Service.qml");
    assert_eq!(manifest["entryPoints"]["barWidget"], "BarWidget.qml");
    assert_eq!(manifest["barWidget"]["defaultSection"], "right");
    assert_eq!(manifest["barWidget"]["allowMultiple"], false);

    let entry_points = manifest["entryPoints"]
        .as_object()
        .expect("entryPoints must be an object");
    assert_eq!(entry_points.len(), 2);
    for entry_point in entry_points.values() {
        let entry_point = entry_point.as_str().expect("entry point must be text");
        assert!(!entry_point.starts_with('/'));
        assert!(!entry_point.contains(".."));
        assert!(workspace_path(entry_point).is_file());
    }
}

#[test]
fn service_is_the_only_process_owner() {
    let service = read("Service.qml");
    let widget = read("BarWidget.qml");

    for required in [
        "import Quickshell.Io",
        "property var manifest: null",
        "readonly property string pluginDir:",
        "function launch()",
        "function stop()",
        "function focus()",
        "Process {",
        "id: supervisorProcess",
        "root.pluginDir + \"/bin/omarchy-adult-content-filter\"",
        "onExited:",
    ] {
        assert!(service.contains(required), "service missing {required:?}");
    }
    for forbidden in ["/usr/bin/chromium", "sudo", "pkexec", "systemctl"] {
        assert!(
            !service.contains(forbidden),
            "service contains forbidden runtime behavior {forbidden:?}"
        );
    }

    assert!(widget.contains("shell.serviceFor(root.pluginId)"));
    assert!(widget.contains("service.launch()"));
    assert!(widget.contains("service.stop()"));
    assert!(!widget.contains("Process {"));
    assert!(!widget.contains("execDetached"));
    assert!(!widget.contains("chromium"));
}

#[test]
fn qml_lifecycle_is_bound_to_the_tested_state_model() {
    let service = read("Service.qml");

    assert!(service.contains("import \"plugin/RuntimeModel.js\" as RuntimeModel"));
    assert!(service.contains("RuntimeModel.requestLaunch(supervisorProcess.running)"));
    assert!(service.contains("RuntimeModel.requestStop(supervisorProcess.running)"));
    assert!(service.contains("RuntimeModel.finishSupervisor(exitCode, root.stopping)"));
}
