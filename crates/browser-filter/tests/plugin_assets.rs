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

#[test]
fn service_exposes_only_supervisor_lifecycle_over_ipc() {
    let service = read("Service.qml");

    for required in [
        "readonly property string pluginId:",
        "IpcHandler {",
        "target: root.pluginId",
        "function launch(): string",
        "function stop(): string",
        "function status(): string",
        "return root.launch() ? \"started\"",
        "return root.stop() ? \"stopping\" : \"stopped\"",
    ] {
        assert!(
            service.contains(required),
            "service IPC missing {required:?}"
        );
    }

    assert_eq!(service.matches("IpcHandler {").count(), 1);
    for forbidden in ["function exec", "function command", "function browse"] {
        assert!(
            !service.contains(forbidden),
            "service IPC exposes {forbidden:?}"
        );
    }
}

#[test]
fn marketplace_surface_documents_one_clone_install_and_honest_scope() {
    let readme = read("README.md");

    for required in [
        "omarchy plugin add https://github.com/jaketothepast/omarchy-adult-content-filter",
        "omarchy plugin enable io.github.jaketothepast.adult-content-filter",
        "omarchy plugin remove io.github.jaketothepast.adult-content-filter",
        "Everything except Chromium is bundled in this plugin repository",
        "browser-only protection",
        "does not prevent a user from launching another browser",
        "does not install a system service",
    ] {
        assert!(readme.contains(required), "README missing {required:?}");
    }

    for forbidden in [
        "separate redistribution review before public release",
        "private evaluation",
        "non-redistributable",
    ] {
        assert!(
            !readme.contains(forbidden),
            "README contains stale release boundary {forbidden:?}"
        );
    }
}

#[test]
fn marketplace_root_contains_public_license_notices_and_preview() {
    let license = read("LICENSE");
    let notices = read("THIRD_PARTY_NOTICES.md");
    assert!(license.contains("GNU AFFERO GENERAL PUBLIC LICENSE"));
    for required in [
        "ONNX Runtime 1.27.1",
        "StevenBlack/hosts",
        "NudeNet",
        "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f",
    ] {
        assert!(notices.contains(required), "notices missing {required:?}");
    }

    let preview = fs::read(workspace_path("preview.png")).expect("preview.png must exist");
    assert!(preview.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(
        preview.len() <= 2 * 1024 * 1024,
        "preview must stay marketplace-sized"
    );
}

#[test]
fn marketplace_plugin_tree_has_no_tracked_symlinks() {
    fn collect_symlinks(directory: &std::path::Path, symlinks: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("plugin directory must be readable") {
            let entry = entry.expect("plugin entry must be readable");
            let path = entry.path();
            let name = entry.file_name();
            if [".git", ".superpowers", ".worktrees", "target", "result"]
                .iter()
                .any(|ignored| name == *ignored)
            {
                continue;
            }
            let file_type = entry
                .file_type()
                .expect("plugin entry type must be readable");
            if file_type.is_symlink() {
                symlinks.push(path);
            } else if file_type.is_dir() {
                collect_symlinks(&path, symlinks);
            }
        }
    }

    let mut symlinks = Vec::new();
    collect_symlinks(&workspace_path("."), &mut symlinks);
    assert!(
        symlinks.is_empty(),
        "the Omarchy marketplace rejects plugin-folder symlinks: {symlinks:?}"
    );
}

#[test]
fn marketplace_discovers_only_the_root_plugin_manifest() {
    let root = workspace_path(".");
    let mut manifests = Vec::new();
    for entry in fs::read_dir(&root).expect("repository root must be readable") {
        let entry = entry.expect("repository root entry must be readable");
        let path = entry.path();
        if entry
            .file_type()
            .expect("entry type must be readable")
            .is_file()
            && entry.file_name() == "manifest.json"
        {
            manifests.push("manifest.json".to_owned());
        } else if entry
            .file_type()
            .expect("entry type must be readable")
            .is_dir()
        {
            let nested = path.join("manifest.json");
            if nested.is_file() {
                manifests.push(format!(
                    "{}/manifest.json",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }
    manifests.sort();
    assert_eq!(manifests, ["manifest.json"]);
}

#[test]
fn marketplace_surface_avoids_unrelated_privilege_and_installer_capabilities() {
    let readme = read("README.md");
    assert!(
        !readme.contains("sudo"),
        "browser-only documentation must not look like a privilege request"
    );

    let runtime_licenses = workspace_path("runtime/share/licenses");
    let setup_files = fs::read_dir(runtime_licenses)
        .expect("runtime licenses must be readable")
        .map(|entry| entry.expect("license entry must be readable").file_name())
        .filter(|name| name.to_string_lossy().ends_with("setup.py"))
        .collect::<Vec<_>>();
    assert!(
        setup_files.is_empty(),
        "license evidence must not be exposed as an executable installer: {setup_files:?}"
    );
    assert!(workspace_path("runtime/share/licenses/nudenet-package-metadata.txt").is_file());
}
