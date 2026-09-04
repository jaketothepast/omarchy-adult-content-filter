use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct WorkspaceFixture {
    _temporary_directory: tempfile::TempDir,
    root: PathBuf,
    primary_checkout: PathBuf,
    linked_worktree: PathBuf,
}

impl WorkspaceFixture {
    fn new() -> Self {
        let temporary_directory = tempfile::tempdir().unwrap();
        let root = temporary_directory.path().to_path_buf();
        let primary_checkout = root.join("omarchy-kids");
        fs::create_dir(&primary_checkout).unwrap();
        run_git(&primary_checkout, &["init", "--quiet"]);
        run_git(&primary_checkout, &["config", "user.name", "Test User"]);
        run_git(
            &primary_checkout,
            &["config", "user.email", "test@example.invalid"],
        );
        fs::write(primary_checkout.join("README.md"), "fixture\n").unwrap();
        run_git(&primary_checkout, &["add", "README.md"]);
        run_git(&primary_checkout, &["commit", "--quiet", "-m", "fixture"]);

        let linked_worktree = primary_checkout.join(".worktrees/feature");
        fs::create_dir(primary_checkout.join(".worktrees")).unwrap();
        run_git(
            &primary_checkout,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "fixture-feature",
                linked_worktree.to_str().unwrap(),
            ],
        );

        Self {
            _temporary_directory: temporary_directory,
            root,
            primary_checkout,
            linked_worktree,
        }
    }

    fn create_default_contracts(&self) -> WorkspacePaths {
        WorkspacePaths::create_at(&self.root)
    }
}

struct WorkspacePaths {
    omarchy: PathBuf,
    iso: PathBuf,
    packages: PathBuf,
}

impl WorkspacePaths {
    fn create_at(root: &Path) -> Self {
        let paths = Self {
            omarchy: root.join("omarchy"),
            iso: root.join("omarchy-iso"),
            packages: root.join("omarchy-pkgs"),
        };
        write_file(&paths.omarchy.join("install/omarchy-base.packages"), "");
        write_bash_executable(&paths.iso.join("bin/omarchy-iso-make"), "exit 0\n");
        write_file(&paths.packages.join("pkgbuilds/omarchy-dev/PKGBUILD"), "");
        paths
    }

    fn environment(&self) -> [(&str, &Path); 3] {
        [
            ("OMARCHY_PATH", &self.omarchy),
            ("OMARCHY_ISO_PATH", &self.iso),
            ("OMARCHY_PKGS_PATH", &self.packages),
        ]
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn script(name: &str) -> PathBuf {
    repository_root().join("scripts").join(name)
}

fn run_git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn write_executable(path: &Path, contents: &str) {
    write_file(path, contents);
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_bash_executable(path: &Path, body: &str) {
    let output = Command::new("bash")
        .arg("-c")
        .arg("printf '%s' \"$BASH\"")
        .output()
        .unwrap();
    assert!(output.status.success());
    let bash = String::from_utf8(output.stdout).unwrap();
    write_executable(path, &format!("#!{bash}\n{body}"));
}

fn resolver_output(current_directory: &Path, environment: &[(&str, &Path)]) -> Output {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(
            r#"set -euo pipefail
source "$1"
printf '%s\0%s\0%s\0' "$OMARCHY_PATH" "$OMARCHY_ISO_PATH" "$OMARCHY_PKGS_PATH""#,
        )
        .arg("resolver-test")
        .arg(script("resolve-workspace"))
        .current_dir(current_directory)
        .env_remove("OMARCHY_PATH")
        .env_remove("OMARCHY_ISO_PATH")
        .env_remove("OMARCHY_PKGS_PATH");
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn resolved_paths(output: &Output) -> Vec<PathBuf> {
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(String::from_utf8(value.to_vec()).unwrap()))
        .collect()
}

fn run_wrapper(
    name: &str,
    workspace: &WorkspacePaths,
    arguments: &[&str],
    extra_environment: &[(&str, &Path)],
) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(script(name))
        .current_dir(repository_root())
        .args(arguments);
    for (variable, value) in workspace.environment() {
        command.env(variable, value);
    }
    for (variable, value) in extra_environment {
        command.env(variable, value);
    }
    command.output().unwrap()
}

fn prepend_path(command: &mut Command, directory: &Path) {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![directory.to_path_buf()];
    paths.extend(std::env::split_paths(&inherited));
    command.env("PATH", std::env::join_paths(paths).unwrap());
}

fn stdout_lines(output: &Output) -> Vec<String> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

const MODEL_SHA256: &str = "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f";

const REQUIRED_INSTALLED_FILES: &[&str] = &[
    "/usr/bin/omarchy-kids-browser-filter-demo",
    "/usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter",
    "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/manifest.json",
    "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/cover.css",
    "/usr/share/applications/omarchy-kids-browser-filter-demo.desktop",
    "/usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx",
    "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-LICENSE",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-ThirdPartyNotices.txt",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/NOTICES.md",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-LICENSE",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-setup.py",
];

const REQUIRED_INSTALLED_LINKS: &[(&str, &str)] = &[
    (
        "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1",
        "libonnxruntime.so.1.27.1",
    ),
    (
        "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so",
        "libonnxruntime.so.1",
    ),
];

const INSTALLED_PACKAGE_PATHS: &[&str] = &[
    "/usr/",
    "/usr/bin/",
    "/usr/bin/omarchy-kids-browser-filter-demo",
    "/usr/lib/",
    "/usr/lib/omarchy-kids-browser-filter-demo/",
    "/usr/lib/omarchy-kids-browser-filter-demo/omarchy-kids-browser-filter",
    "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/",
    "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so",
    "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1",
    "/usr/lib/omarchy-kids-browser-filter-demo/onnxruntime/libonnxruntime.so.1.27.1",
    "/usr/share/",
    "/usr/share/applications/",
    "/usr/share/applications/omarchy-kids-browser-filter-demo.desktop",
    "/usr/share/licenses/",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/NOTICES.md",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-LICENSE",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/nudenet-setup.py",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-LICENSE",
    "/usr/share/licenses/omarchy-kids-browser-filter-demo/onnxruntime-ThirdPartyNotices.txt",
    "/usr/share/omarchy-kids-browser-filter-demo/",
    "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/",
    "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/cover.css",
    "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/manifest.json",
    "/usr/share/omarchy-kids-browser-filter-demo/models/",
    "/usr/share/omarchy-kids-browser-filter-demo/models/320n.onnx",
];

struct GuestAcceptanceFixture {
    _temporary_directory: tempfile::TempDir,
    root: PathBuf,
    install_root: PathBuf,
    artifacts: PathBuf,
    fake_bin: PathBuf,
    profile_root: PathBuf,
    summary: PathBuf,
    metrics: PathBuf,
    client_state: PathBuf,
    process_state: PathBuf,
    package_paths: PathBuf,
}

impl GuestAcceptanceFixture {
    fn new() -> Self {
        Self::with_missing_path(None)
    }

    fn with_missing_path(missing: Option<&str>) -> Self {
        let temporary_directory = tempfile::tempdir().unwrap();
        let root = temporary_directory.path().to_path_buf();
        let install_root = root.join("installed root");
        let artifacts = root.join("acceptance artifacts");
        let fake_bin = root.join("fake bin");
        let profile_root = root.join("profiles");
        let summary = root.join("launcher summary.json");
        let metrics = root.join("launcher metrics.jsonl");
        let client_state = root.join("client state");
        let process_state = root.join("process state");
        let package_paths = root.join("package paths");

        for path in [&artifacts, &fake_bin, &profile_root] {
            fs::create_dir_all(path).unwrap();
        }
        write_file(
            &artifacts.join("acceptance.log"),
            "external acceptance started\n",
        );

        for relative in REQUIRED_INSTALLED_FILES {
            if missing == Some(*relative) {
                continue;
            }
            let path = Self::rooted(&install_root, relative);
            if relative.ends_with("omarchy-kids-browser-filter-demo")
                || relative.ends_with("omarchy-kids-browser-filter")
                || relative.ends_with("libonnxruntime.so.1.27.1")
            {
                write_executable(&path, "fixture\n");
            } else {
                write_file(&path, "fixture\n");
            }
        }
        let extension = Self::rooted(
            &install_root,
            "/usr/share/omarchy-kids-browser-filter-demo/browser-extension",
        );
        fs::create_dir_all(&extension).unwrap();
        for (relative, target) in REQUIRED_INSTALLED_LINKS {
            if missing == Some(*relative) {
                continue;
            }
            let path = Self::rooted(&install_root, relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, path).unwrap();
        }

        write_file(&summary, &valid_guest_summary());
        write_file(&metrics, &valid_guest_metrics());
        write_file(&client_state, "");
        write_file(&process_state, "111\n");
        write_file(&package_paths, &(INSTALLED_PACKAGE_PATHS.join("\n") + "\n"));

        write_bash_executable(
            &fake_bin.join("pacman"),
            r#"if [[ $# == 2 && $1 == "-Q" && $2 == "omarchy-kids-browser-filter-demo" ]]; then
  [[ ${FAKE_PACKAGE_PRESENT:-1} == 1 ]]
elif [[ $# == 2 && $1 == "-Qlq" && $2 == "omarchy-kids-browser-filter-demo" ]]; then
  cat "$FAKE_PACKAGE_PATHS"
elif [[ $# == 2 && $1 == "-Qqo" ]]; then
  printf '%s\n' "${FAKE_PATH_OWNER:-omarchy-kids-browser-filter-demo}"
else
  exit 64
fi
"#,
        );
        write_bash_executable(
            &fake_bin.join("stat"),
            r#"path=${@: -1}
if [[ ${FAKE_BAD_METADATA:-0} == 1 && $path == */omarchy-kids-browser-filter-demo.desktop ]]; then
  printf 'root:root 600\n'
  exit 0
fi
case "$path" in
  */omarchy-kids-browser-filter-demo|*/omarchy-kids-browser-filter|*/libonnxruntime.so.1.27.1)
    printf 'root:root 755\n'
    ;;
  */browser-extension)
    printf 'root:root 755\n'
    ;;
  */libonnxruntime.so|*/libonnxruntime.so.1)
    printf 'root:root 777\n'
    ;;
  *)
    printf 'root:root 644\n'
    ;;
esac
"#,
        );
        write_bash_executable(
            &fake_bin.join("sha256sum"),
            "printf '%s  %s\\n' \"${FAKE_MODEL_HASH}\" \"$1\"\n",
        );
        write_bash_executable(
            &fake_bin.join("hyprctl"),
            r#"[[ $1 == "-j" && $2 == "clients" ]] || exit 64
if [[ ${FAKE_OBSERVE_CLIENT:-1} == 1 && -s $FAKE_CLIENT_STATE ]]; then
  printf '[{"address":"0xold","class":"Chromium"},{"address":"0xnew","class":"chromium"}]\n'
else
  printf '[{"address":"0xold","class":"Chromium"}]\n'
fi
"#,
        );
        write_bash_executable(
            &fake_bin.join("pgrep"),
            r#"if [[ $# == 3 && $1 == "-f" && $2 == "--" && $3 == "chromium.*omarchy-kids-browser-" ]]; then
  cat "$FAKE_PROCESS_STATE"
else
  exit 64
fi
"#,
        );
        write_bash_executable(
            &fake_bin.join("omarchy-kids-browser-filter-demo"),
            r#"expected=(--images 17 --flagged-index 5 --hold-millis 1500 --assert-no-flash --json)
actual=("$@")
[[ $# == ${#expected[@]} ]] || exit 91
for index in "${!expected[@]}"; do
  [[ ${actual[index]} == "${expected[index]}" ]] || exit 92
done
printf 'running\n' >"$FAKE_CLIENT_STATE"
printf '111\n222\n' >"$FAKE_PROCESS_STATE"
cat "$FAKE_SUMMARY"
cat "$FAKE_METRICS" >&2
sleep 0.3
printf '' >"$FAKE_CLIENT_STATE"
if [[ ${FAKE_LEAK_PROCESS:-0} == 0 ]]; then
  printf '111\n' >"$FAKE_PROCESS_STATE"
fi
if [[ ${FAKE_LEAK_PROFILE:-0} == 1 ]]; then
  mkdir -p "$FAKE_PROFILE_ROOT/omarchy-kids-browser-leaked"
fi
exit "${FAKE_LAUNCHER_STATUS:-0}"
"#,
        );

        Self {
            _temporary_directory: temporary_directory,
            root,
            install_root,
            artifacts,
            fake_bin,
            profile_root,
            summary,
            metrics,
            client_state,
            process_state,
            package_paths,
        }
    }

    fn rooted(root: &Path, installed_path: &str) -> PathBuf {
        root.join(installed_path.trim_start_matches('/'))
    }

    fn run(&self, extra_environment: &[(&str, &str)]) -> Output {
        let mut command = Command::new("bash");
        command
            .arg(repository_root().join("test/acceptance.d/browser-filter-demo-test.sh"))
            .current_dir(repository_root())
            .env("HOME", &self.root)
            .env("OMARCHY_ACCEPTANCE_DIR", &self.artifacts)
            .env("OMARCHY_KIDS_INSTALL_ROOT", &self.install_root)
            .env("OMARCHY_KIDS_PROFILE_ROOT", &self.profile_root)
            .env("OMARCHY_KIDS_CLIENT_TIMEOUT", "2")
            .env("OMARCHY_KIDS_CLIENT_POLL_SECONDS", "0.05")
            .env("FAKE_PACKAGE_PRESENT", "1")
            .env("FAKE_PATH_OWNER", "omarchy-kids-browser-filter-demo")
            .env("FAKE_BAD_METADATA", "0")
            .env("FAKE_MODEL_HASH", MODEL_SHA256)
            .env("FAKE_OBSERVE_CLIENT", "1")
            .env("FAKE_LEAK_PROCESS", "0")
            .env("FAKE_LEAK_PROFILE", "0")
            .env("FAKE_LAUNCHER_STATUS", "0")
            .env("FAKE_SUMMARY", &self.summary)
            .env("FAKE_METRICS", &self.metrics)
            .env("FAKE_CLIENT_STATE", &self.client_state)
            .env("FAKE_PROCESS_STATE", &self.process_state)
            .env("FAKE_PACKAGE_PATHS", &self.package_paths)
            .env("FAKE_PROFILE_ROOT", &self.profile_root);
        prepend_path(&mut command, &self.fake_bin);
        for (name, value) in extra_environment {
            command.env(name, value);
        }
        command.output().unwrap()
    }
}

fn valid_guest_summary() -> String {
    let dom_images = (0..17)
        .map(|index| {
            if index == 5 {
                r#"{"rgba":[255,0,255,255]}"#.to_owned()
            } else {
                "{}".to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"intercepted":17,"continued":16,"replaced":1,"unresolved":0,"clean_shutdown":true,"onnx_runtime_version":"1.27.1","model_sha256":"{MODEL_SHA256}","reveal_latency_millis":500,"no_flash_assertion":{{"requested_hold_millis":1500,"actual_hold_millis":1500,"hold_screenshot_count":3,"hold_sampled_pixels":1,"cover_rgba":[17,19,24,255],"safe_fixture_colors_present":16,"placeholder_color_present":true,"original_flagged_color_absent":true}},"dom_images":[{dom_images}]}}"#
    ) + "\n"
}

fn valid_guest_metrics() -> String {
    (0..34)
        .map(|index| {
            format!(
                "{{\"elapsed_micros\":{},\"fixture_index\":{},\"stage\":\"inference\",\"verdict\":\"allow\"}}\n",
                index + 1,
                index / 2
            )
        })
        .collect()
}

// Production mutation caught: deriving defaults from the linked worktree places siblings
// beneath .worktrees/feature instead of beside the primary checkout.
#[test]
fn resolver_derives_defaults_from_the_primary_checkout() {
    let fixture = WorkspaceFixture::new();
    let paths = fixture.create_default_contracts();

    let output = resolver_output(&fixture.linked_worktree, &[]);

    assert!(
        output.status.success(),
        "resolver failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        resolved_paths(&output),
        vec![paths.omarchy, paths.iso, paths.packages]
    );
    assert!(
        !fixture
            .primary_checkout
            .starts_with(&fixture.linked_worktree)
    );
}

// Production mutation caught: accepting an incomplete checkout lets a later workflow fail
// without identifying the checkout contract or the environment override that repairs it.
#[test]
fn resolver_reports_exact_remediation_for_each_missing_checkout() {
    let cases = [
        (
            "omarchy",
            "install/omarchy-base.packages",
            "OMARCHY_PATH",
            "Omarchy",
        ),
        (
            "omarchy-iso",
            "bin/omarchy-iso-make",
            "OMARCHY_ISO_PATH",
            "Omarchy ISO",
        ),
        (
            "omarchy-pkgs",
            "pkgbuilds/omarchy-dev/PKGBUILD",
            "OMARCHY_PKGS_PATH",
            "Omarchy packages",
        ),
    ];

    for (checkout, required_file, variable, label) in cases {
        let fixture = WorkspaceFixture::new();
        fixture.create_default_contracts();
        fs::remove_file(fixture.root.join(checkout).join(required_file)).unwrap();

        let output = resolver_output(&fixture.linked_worktree, &[]);

        assert!(
            !output.status.success(),
            "missing {checkout} unexpectedly passed"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!(
                "ERROR {checkout}: expected {}/{}.\nSet {variable} to the {label} checkout.\n",
                fixture.root.join(checkout).display(),
                required_file
            )
        );
    }
}

// Production mutation caught: skipping realpath preserves .. components and makes downstream
// Docker mounts and command paths depend on the caller's spelling.
#[test]
fn resolver_canonicalizes_valid_environment_paths() {
    let fixture = WorkspaceFixture::new();
    let paths = WorkspacePaths::create_at(&fixture.root.join("overrides"));
    let spelled_omarchy = fixture.root.join("overrides/../overrides/omarchy");
    let spelled_iso = fixture.root.join("overrides/../overrides/omarchy-iso");
    let spelled_packages = fixture.root.join("overrides/../overrides/omarchy-pkgs");
    let environment = [
        ("OMARCHY_PATH", spelled_omarchy.as_path()),
        ("OMARCHY_ISO_PATH", spelled_iso.as_path()),
        ("OMARCHY_PKGS_PATH", spelled_packages.as_path()),
    ];

    let output = resolver_output(&fixture.linked_worktree, &environment);

    assert!(
        output.status.success(),
        "resolver failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        resolved_paths(&output),
        vec![
            paths.omarchy.canonicalize().unwrap(),
            paths.iso.canonicalize().unwrap(),
            paths.packages.canonicalize().unwrap(),
        ]
    );
}

// Production mutation caught: preferring sibling defaults ignores an explicitly selected set
// of checkouts and can build an ISO from the wrong source tree.
#[test]
fn resolver_environment_overrides_win_over_valid_defaults() {
    let fixture = WorkspaceFixture::new();
    fixture.create_default_contracts();
    let overrides = WorkspacePaths::create_at(&fixture.root.join("selected"));

    let output = resolver_output(&fixture.linked_worktree, &overrides.environment());

    assert!(output.status.success());
    assert_eq!(
        resolved_paths(&output),
        vec![overrides.omarchy, overrides.iso, overrides.packages]
    );
}

// Production mutation caught: requiring Git even when every checkout is explicit prevents the
// wrappers from running in Git-free packaged or test environments where no default is needed.
#[test]
fn resolver_accepts_complete_overrides_outside_a_git_checkout() {
    let fixture = tempfile::tempdir().unwrap();
    let paths = WorkspacePaths::create_at(&fixture.path().join("selected"));
    let outside_git = fixture.path().join("not-a-checkout");
    fs::create_dir(&outside_git).unwrap();

    let output = resolver_output(&outside_git, &paths.environment());

    assert!(
        output.status.success(),
        "resolver failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        resolved_paths(&output),
        vec![paths.omarchy, paths.iso, paths.packages]
    );
}

// Production mutation caught: an early exit or omitted prerequisite can hide multiple reasons
// that the ISO workflow is not ready, forcing repeated doctor runs to discover them.
#[test]
fn doctor_reports_each_check_once_and_aggregates_failures() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let fake_bin = fixture.path().join("fake-bin");
    fs::create_dir(&fake_bin).unwrap();
    write_bash_executable(&fake_bin.join("docker"), "[[ ${1:-} != info ]]\n");
    write_bash_executable(
        &fake_bin.join("df"),
        "printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\\nfake 50000000 0 50000000 0%% /\\n'\n",
    );
    let missing_chromium = fixture.path().join("missing-chromium");
    let missing_onnx = fixture.path().join("missing-onnx.so");
    let missing_model = fixture.path().join("missing-model.onnx");
    let missing_extension = fixture.path().join("missing-extension");
    let missing_code = fixture.path().join("missing-code.fd");
    let missing_vars = fixture.path().join("missing-vars.fd");
    let mut command = Command::new("bash");
    command
        .arg(script("doctor"))
        .current_dir(repository_root())
        .env("CHROMIUM_BIN", &missing_chromium)
        .env("ORT_DYLIB_PATH", &missing_onnx)
        .env("NUDENET_MODEL_PATH", &missing_model)
        .env("OMARCHY_KIDS_EXTENSION_DIR", &missing_extension)
        .env("OMARCHY_VM_OVMF_CODE", &missing_code)
        .env("OMARCHY_VM_OVMF_VARS_TEMPLATE", &missing_vars)
        .env("OMARCHY_KIDS_WORKSPACE_PARENT", fixture.path());
    for (variable, value) in workspace.environment() {
        command.env(variable, value);
    }
    prepend_path(&mut command, &fake_bin);

    let output = command.output().unwrap();

    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let lines = stdout_lines(&output);
    assert_eq!(lines.len(), 13, "doctor output: {lines:#?}");
    let labels: Vec<&str> = lines
        .iter()
        .map(|line| line.split_once(' ').unwrap().1.split(':').next().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "chromium",
            "onnx-runtime",
            "nudenet-model",
            "extension",
            "docker-client",
            "docker-server",
            "kvm",
            "ovmf-code",
            "ovmf-vars-template",
            "disk-space",
            "omarchy-checkout",
            "omarchy-iso-checkout",
            "omarchy-pkgs-checkout",
        ]
    );
    assert_eq!(
        lines[0],
        format!(
            "FAIL chromium: not executable: {}",
            missing_chromium.display()
        )
    );
    assert_eq!(
        lines[1],
        format!("FAIL onnx-runtime: not found: {}", missing_onnx.display())
    );
    assert_eq!(
        lines[2],
        format!("FAIL nudenet-model: not found: {}", missing_model.display())
    );
    assert_eq!(
        lines[3],
        format!(
            "FAIL extension: manifest not found: {}/manifest.json",
            missing_extension.display()
        )
    );
    assert_eq!(lines[4], "PASS docker-client: available");
    assert_eq!(lines[5], "FAIL docker-server: daemon unavailable");
    assert!(matches!(
        lines[6].as_str(),
        "PASS kvm: writable" | "FAIL kvm: /dev/kvm is not writable"
    ));
    assert_eq!(
        lines[7],
        format!("FAIL ovmf-code: not found: {}", missing_code.display())
    );
    assert_eq!(
        lines[8],
        format!(
            "FAIL ovmf-vars-template: not found: {}",
            missing_vars.display()
        )
    );
    assert_eq!(lines[9], "PASS disk-space: at least 40 GiB available");
    assert_eq!(lines[10], "PASS omarchy-checkout: contract satisfied");
    assert_eq!(lines[11], "PASS omarchy-iso-checkout: contract satisfied");
    assert_eq!(lines[12], "PASS omarchy-pkgs-checkout: contract satisfied");
}

// Production mutation caught: calling any other test entry point, or adding arguments, no
// longer runs the ISO repository's complete VM-free unit suite exactly as provided.
#[test]
fn iso_unit_runs_the_iso_test_all_entry_point() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("test/all"),
        "printf 'iso-unit:%s\\n' \"$#\"\n",
    );

    let output = run_wrapper("iso-unit", &workspace, &[], &[]);

    assert!(output.status.success());
    assert_eq!(stdout_lines(&output), ["iso-unit:0"]);
}

// Production mutation caught: changing flag order or expanding "$@" loses the unattended
// build contract or splits caller arguments containing whitespace.
#[test]
fn iso_build_forwards_exact_flags_and_arguments_in_order() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-make"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );

    let output = run_wrapper(
        "iso-build",
        &workspace,
        &["--debug", "value with spaces", ""],
        &[],
    );

    assert!(
        output.status.success(),
        "iso-build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            "ARG=--keep-pkg-cache",
            "ARG=--no-boot-offer",
            "ARG=--local-source",
            &format!("ARG={}", workspace.omarchy.display()),
            &format!("ARG={}", workspace.packages.display()),
            "ARG=--debug",
            "ARG=value with spaces",
            "ARG=",
        ]
    );
}

// Production mutation caught: changing fixed option order, omitting local package input, or
// splitting a source path with spaces changes the reviewed ISO builder contract.
#[test]
fn kids_iso_build_forwards_exact_local_package_contract() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-make"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );
    let tag = Path::new("kids-demo-contract");

    let output = run_wrapper(
        "kids-iso-build",
        &workspace,
        &[],
        &[
            ("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path()),
            ("OMARCHY_KIDS_ISO_TAG", tag),
        ],
    );

    assert!(
        output.status.success(),
        "kids ISO build wrapper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            "ARG=--keep-pkg-cache",
            "ARG=--no-boot-offer",
            "ARG=--local-source",
            &format!("ARG={}", workspace.omarchy.display()),
            &format!("ARG={}", workspace.packages.display()),
            "ARG=--local-package",
            "ARG=omarchy-kids-browser-filter-demo",
            &format!("ARG={}", package_source.display()),
            "ARG=--output-tag",
            "ARG=kids-demo-contract",
        ]
    );
}

// Production mutation caught: delegating an absent Git-indexed source lets the ISO builder fail
// later without identifying the Kids package-source boundary.
#[test]
fn kids_iso_build_rejects_a_missing_package_source_before_delegation() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-make"),
        "printf 'delegated\\n'\n",
    );
    let missing_source = fixture.path().join("missing package source");

    let output = run_wrapper(
        "kids-iso-build",
        &workspace,
        &[],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", missing_source.as_path())],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "builder was delegated to");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "ERROR kids-iso-build: package source not found: {}\n",
            missing_source.display()
        )
    );
}

#[test]
fn adult_filter_iso_build_forwards_the_product_package_and_unique_tag() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let package_source = fixture.path().join("adult filter source");
    fs::create_dir(&package_source).unwrap();
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-make"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );
    let tag = Path::new("adult-filter-contract");

    let output = run_wrapper(
        "adult-filter-iso-build",
        &workspace,
        &[],
        &[
            (
                "OMARCHY_ADULT_FILTER_PACKAGE_SOURCE",
                package_source.as_path(),
            ),
            ("OMARCHY_ADULT_FILTER_ISO_TAG", tag),
        ],
    );

    assert!(
        output.status.success(),
        "adult-filter ISO build wrapper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            "ARG=--keep-pkg-cache",
            "ARG=--no-boot-offer",
            "ARG=--local-source",
            &format!("ARG={}", workspace.omarchy.display()),
            &format!("ARG={}", workspace.packages.display()),
            "ARG=--local-package",
            "ARG=omarchy-adult-content-filter",
            &format!("ARG={}", package_source.display()),
            "ARG=--output-tag",
            "ARG=adult-filter-contract",
        ]
    );
}

#[test]
fn adult_filter_iso_test_owns_one_external_suite_and_preserves_arguments() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let fake = r#"printf 'MANAGE=%s\n' "${OMARCHY_ISO_MANAGE_HOST_DEPS-unset}"
printf 'ARG=%s\n' "$@"
"#;
    write_bash_executable(&workspace.iso.join("bin/omarchy-iso-test"), fake);
    let iso = fixture.path().join("artifacts/adult filter.iso");
    write_file(&iso, "fixture");
    let package_source = fixture.path().join("adult filter source");
    fs::create_dir(&package_source).unwrap();

    let output = run_wrapper(
        "adult-filter-iso-test",
        &workspace,
        &[iso.to_str().unwrap(), "--reuse-base", "--no-preview"],
        &[(
            "OMARCHY_ADULT_FILTER_PACKAGE_SOURCE",
            package_source.as_path(),
        )],
    );

    assert!(
        output.status.success(),
        "adult-filter ISO test wrapper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            "MANAGE=0",
            &format!("ARG={}", iso.canonicalize().unwrap().display()),
            "ARG=--refuse-existing-base",
            "ARG=--external-acceptance",
            &format!("ARG={}", package_source.display()),
            "ARG=--reuse-base",
            "ARG=--no-preview",
        ]
    );

    let injected = run_wrapper(
        "adult-filter-iso-test",
        &workspace,
        &[
            iso.to_str().unwrap(),
            "--external-acceptance",
            package_source.to_str().unwrap(),
        ],
        &[(
            "OMARCHY_ADULT_FILTER_PACKAGE_SOURCE",
            package_source.as_path(),
        )],
    );
    assert!(!injected.status.success());
    assert!(injected.stdout.is_empty());
    assert_eq!(
        String::from_utf8(injected.stderr).unwrap(),
        "ERROR adult-filter-iso-test: --external-acceptance is managed by this wrapper\n"
    );
}

#[test]
fn adult_filter_acceptance_entry_points_are_product_named_and_browser_only() {
    let runner = fs::read_to_string(repository_root().join("test/acceptance"))
        .expect("external acceptance runner is missing");
    assert!(runner.contains("test/acceptance.d/adult-content-filter-test.sh"));
    assert!(!runner.contains("browser-filter-demo-test.sh"));

    let suite = fs::read_to_string(
        repository_root().join("test/acceptance.d/adult-content-filter-test.sh"),
    )
    .expect("adult-filter acceptance suite is missing");
    for required in [
        "PACKAGE=omarchy-adult-content-filter",
        "/usr/bin/omarchy-adult-content-filter",
        "/usr/lib/omarchy-adult-content-filter/omarchy-adult-content-filter",
        "/usr/share/omarchy-adult-content-filter/policies/adult-domains.hosts",
        "managed browser launcher exits successfully",
        "controlled detector proof exits successfully",
    ] {
        assert!(suite.contains(required), "missing {required:?}");
    }
    for forbidden in [
        "useradd",
        "passwd",
        "sudoers",
        "systemctl enable",
        "/etc/chromium/policies",
    ] {
        assert!(!suite.contains(forbidden), "unexpected {forbidden:?}");
    }

    let apps = fs::read_to_string(repository_root().join("nix/apps.nix"))
        .expect("Nix app definitions are missing");
    for required in [
        "adult-filter-iso-build = mkApp",
        "command = scriptCommand \"adult-filter-iso-build\"",
        "adult-filter-iso-test = mkApp",
        "command = scriptCommand \"adult-filter-iso-test\"",
        "OMARCHY_ADULT_FILTER_PACKAGE_SOURCE = source",
    ] {
        assert!(apps.contains(required), "missing {required:?}");
    }
    assert!(!apps.contains("kids-iso-build = mkApp"));
    assert!(!apps.contains("kids-iso-test = mkApp"));

    let flake = fs::read_to_string(repository_root().join("flake.nix"))
        .expect("flake definition is missing");
    assert!(flake.contains("Omarchy Adult Content Filter"));
    let readme =
        fs::read_to_string(repository_root().join("README.md")).expect("README is missing");
    assert!(readme.starts_with("# Omarchy Adult Content Filter\n"));
    assert!(readme.contains("browser-only"));
    assert!(readme.contains("does not prevent a user from launching another browser"));
}

// Production mutation caught: falling through to the upstream interactive selector makes a
// supposedly explicit workflow nondeterministic when no ISO path is supplied.
#[test]
fn vm_wrappers_reject_a_missing_explicit_iso_argument() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());

    for name in ["iso-test", "iso-integration"] {
        let output = run_wrapper(name, &workspace, &[], &[]);

        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("Usage: {name} <path-to.iso> [arguments...]\n")
        );
    }
}

// Production mutation caught: omitting realpath, dependency mode, firmware environment, or
// quoted forwarding changes what the two real QEMU harnesses receive.
#[test]
fn vm_wrappers_canonicalize_iso_and_forward_environment_and_arguments() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let fake = r#"printf 'MANAGE=%s\n' "${OMARCHY_ISO_MANAGE_HOST_DEPS-unset}"
printf 'CODE=%s\n' "${OMARCHY_VM_OVMF_CODE-unset}"
printf 'VARS=%s\n' "${OMARCHY_VM_OVMF_VARS_TEMPLATE-unset}"
printf 'ARG=%s\n' "$@"
"#;
    write_bash_executable(&workspace.iso.join("bin/omarchy-iso-test"), fake);
    write_bash_executable(&workspace.iso.join("test/integration"), fake);
    let iso = fixture.path().join("artifacts/image.iso");
    write_file(&iso, "fixture");
    let spelled_iso = fixture.path().join("artifacts/../artifacts/image.iso");
    let ovmf_code = fixture.path().join("OVMF CODE.fd");
    let ovmf_vars = fixture.path().join("OVMF VARS.fd");
    let environment = [
        ("OMARCHY_VM_OVMF_CODE", ovmf_code.as_path()),
        ("OMARCHY_VM_OVMF_VARS_TEMPLATE", ovmf_vars.as_path()),
    ];

    for name in ["iso-test", "iso-integration"] {
        let output = run_wrapper(
            name,
            &workspace,
            &[
                spelled_iso.to_str().unwrap(),
                "--option",
                "value with spaces",
                "",
            ],
            &environment,
        );

        assert!(
            output.status.success(),
            "{name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout_lines(&output),
            [
                "MANAGE=0",
                &format!("CODE={}", ovmf_code.display()),
                &format!("VARS={}", ovmf_vars.display()),
                &format!("ARG={}", iso.display()),
                "ARG=--option",
                "ARG=value with spaces",
                "ARG=",
            ],
            "{name} changed the delegated contract"
        );
    }
}

// Production mutation caught: changing either wrapper-owned option, omitting dependency mode,
// or expanding caller arguments changes what the reviewed real QEMU harness receives.
#[test]
fn kids_iso_test_canonicalizes_and_forwards_the_external_suite_contract() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let fake = r#"printf 'MANAGE=%s\n' "${OMARCHY_ISO_MANAGE_HOST_DEPS-unset}"
printf 'ARG=%s\n' "$@"
"#;
    write_bash_executable(&workspace.iso.join("bin/omarchy-iso-test"), fake);
    let iso = fixture.path().join("artifacts/kids demo.iso");
    write_file(&iso, "fixture");
    let spelled_iso = fixture.path().join("artifacts/../artifacts/kids demo.iso");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[
            spelled_iso.to_str().unwrap(),
            "--reuse-base",
            "--option",
            "value with spaces",
            "",
        ],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(
        output.status.success(),
        "kids ISO test wrapper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            "MANAGE=0",
            &format!("ARG={}", iso.display()),
            "ARG=--refuse-existing-base",
            "ARG=--external-acceptance",
            &format!("ARG={}", package_source.display()),
            "ARG=--reuse-base",
            "ARG=--option",
            "ARG=value with spaces",
            "ARG=",
        ]
    );
}

// Production mutation caught: permitting a caller-owned external suite would inject a second
// hook into the consumer and make the installed Kids acceptance boundary ambiguous.
#[test]
fn kids_iso_test_rejects_caller_supplied_external_acceptance_before_delegation() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'delegated\\n'\n",
    );
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    let injected_source = fixture.path().join("injected source");
    fs::create_dir(&injected_source).unwrap();

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[
            iso.to_str().unwrap(),
            "--external-acceptance",
            injected_source.to_str().unwrap(),
        ],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "ISO harness was delegated to");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "ERROR kids-iso-test: --external-acceptance is managed by this wrapper\n"
    );
}

// Production mutation caught: falling through to the ISO harness's interactive selection makes
// the Kids workflow nondeterministic when no explicit artifact is supplied.
#[test]
fn kids_iso_test_rejects_a_missing_explicit_iso_argument() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Usage: kids-iso-test <path-to.iso> [arguments...]\n"
    );
}

// Production mutation caught: resolving or delegating a nonexistent artifact obscures which ISO
// the caller misspelled and can enter sibling setup before the explicit-input contract is checked.
#[test]
fn kids_iso_test_rejects_a_non_file_iso_before_delegation() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'delegated\\n'\n",
    );
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    let missing_iso = fixture.path().join("missing.iso");

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[missing_iso.to_str().unwrap()],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "ISO harness was delegated to");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "ERROR kids-iso-test: ISO not found: {}\n",
            missing_iso.display()
        )
    );
}

// Production mutation caught: delegating an absent Git-indexed source asks the ISO harness to
// stream a nonexistent external suite after it may already have prepared VM state.
#[test]
fn kids_iso_test_rejects_a_missing_package_source_before_delegation() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'delegated\\n'\n",
    );
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let missing_source = fixture.path().join("missing package source");

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[iso.to_str().unwrap()],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", missing_source.as_path())],
    );

    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "ISO harness was delegated to");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "ERROR kids-iso-test: package source not found: {}\n",
            missing_source.display()
        )
    );
}

// Production mutation caught: wrapper-owned flags must not reorder, consume, or omit valid
// downstream mode arguments.
#[test]
fn kids_iso_test_delegates_each_mode_with_exact_original_arguments_when_no_base_exists() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );
    let cases: &[(&[&str], &[&str])] = &[
        (&["--encrypt"], &["ARG=--encrypt"]),
        (&["--provision"], &["ARG=--provision"]),
        (
            &["--provision", "--encrypt"],
            &["ARG=--provision", "ARG=--encrypt"],
        ),
    ];

    for (mode_arguments, expected_mode_lines) in cases {
        let mut arguments = vec![iso.to_str().unwrap()];
        arguments.extend_from_slice(mode_arguments);
        arguments.push("--no-preview");

        let output = run_wrapper(
            "kids-iso-test",
            &workspace,
            &arguments,
            &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
        );

        assert!(
            output.status.success(),
            "mode {mode_arguments:?} was not delegated: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut expected = vec![
            format!("ARG={}", iso.display()),
            "ARG=--refuse-existing-base".to_owned(),
            "ARG=--external-acceptance".to_owned(),
            format!("ARG={}", package_source.display()),
        ];
        expected.extend(expected_mode_lines.iter().map(|line| (*line).to_owned()));
        expected.push("ARG=--no-preview".to_owned());
        assert_eq!(stdout_lines(&output), expected);
    }
}

// Production mutation caught: parsing downstream values or positional ISO replacements in this
// wrapper can drift from the real consumer and guard a different base than the consumer selects.
#[test]
fn kids_iso_test_leaves_later_positional_iso_and_value_decoy_for_the_consumer() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let alternate_iso = fixture.path().join("alternate demo.iso");
    write_file(&alternate_iso, "alternate fixture");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'ARG=%s\n' \"$@\"\n",
    );

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[
            iso.to_str().unwrap(),
            "--memory",
            "--reuse-base",
            alternate_iso.to_str().unwrap(),
            "--encrypt",
        ],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(
        output.status.success(),
        "downstream arguments were not delegated: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            &format!("ARG={}", iso.display()),
            "ARG=--refuse-existing-base",
            "ARG=--external-acceptance",
            &format!("ARG={}", package_source.display()),
            "ARG=--memory",
            "ARG=--reuse-base",
            &format!("ARG={}", alternate_iso.display()),
            "ARG=--encrypt",
        ]
    );
}
// Production mutation caught: refusing an explicitly selected reusable base breaks the intended
// second-phase acceptance run; deleting it would destroy preserved installation evidence.
#[test]
fn kids_iso_test_preserves_and_delegates_an_explicitly_reused_base() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    let base = workspace.iso.join("test-runs/kids-demo/base.qcow2");
    write_file(&base, "preserved base");

    let output = run_wrapper(
        "kids-iso-test",
        &workspace,
        &[iso.to_str().unwrap(), "--reuse-base", "--no-preview"],
        &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
    );

    assert!(
        output.status.success(),
        "explicit reuse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        stdout_lines(&output),
        [
            &format!("ARG={}", iso.display()),
            "ARG=--refuse-existing-base",
            "ARG=--external-acceptance",
            &format!("ARG={}", package_source.display()),
            "ARG=--reuse-base",
            "ARG=--no-preview",
        ]
    );
    assert_eq!(fs::read_to_string(base).unwrap(), "preserved base");
}

// Production mutation caught: interpreting consumer option arities here can consume, reorder, or
// reject valid downstream values as the consumer evolves.
#[test]
fn kids_iso_test_preserves_each_downstream_value_option_and_later_reuse() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = WorkspacePaths::create_at(fixture.path());
    write_bash_executable(
        &workspace.iso.join("bin/omarchy-iso-test"),
        "printf 'ARG=%s\\n' \"$@\"\n",
    );
    let iso = fixture.path().join("kids-demo.iso");
    write_file(&iso, "fixture");
    let package_source = fixture.path().join("package source");
    fs::create_dir(&package_source).unwrap();
    let base = workspace.iso.join("test-runs/kids-demo/base.qcow2");

    for (value_option, value) in [
        ("--sync-omarchy", "sync source"),
        ("--sync-all", "complete source"),
        ("--port", "2223"),
        ("--memory", "4096"),
        ("--timeout", "1200"),
    ] {
        write_file(&base, "preserved base");
        let output = run_wrapper(
            "kids-iso-test",
            &workspace,
            &[
                iso.to_str().unwrap(),
                value_option,
                value,
                "--reuse-base",
                "--no-preview",
            ],
            &[("OMARCHY_KIDS_PACKAGE_SOURCE", package_source.as_path())],
        );

        assert!(
            output.status.success(),
            "{value_option} hid real reuse: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout_lines(&output),
            [
                &format!("ARG={}", iso.display()),
                "ARG=--refuse-existing-base",
                "ARG=--external-acceptance",
                &format!("ARG={}", package_source.display()),
                &format!("ARG={value_option}"),
                &format!("ARG={value}"),
                "ARG=--reuse-base",
                "ARG=--no-preview",
            ]
        );
        assert_eq!(fs::read_to_string(&base).unwrap(), "preserved base");
    }
}

// Production mutation caught: omitting one package-owned asset lets a partially installed demo
// reach browser launch even though the installed-product contract is incomplete.
#[test]
fn guest_acceptance_rejects_a_missing_required_installed_path() {
    let missing = "/usr/share/applications/omarchy-kids-browser-filter-demo.desktop";
    let fixture = GuestAcceptanceFixture::with_missing_path(Some(missing));

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("not ok - required installed path exists: {missing}\n")
    );
}

// The initial outside-VM RED: a missing package must be the first diagnostic and must prevent all
// installed-path and graphical-session work.
#[test]
fn guest_acceptance_rejects_an_absent_package_first() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_PACKAGE_PRESENT", "0")]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - omarchy-kids-browser-filter-demo package is installed\n"
    );
    assert!(
        fs::read_to_string(&fixture.client_state)
            .unwrap()
            .is_empty()
    );
}

// Production mutation caught: checking only path presence accepts a package asset whose installed
// mode no longer matches the reviewed root-owned package contract.
#[test]
fn guest_acceptance_rejects_wrong_installed_ownership_or_mode() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_BAD_METADATA", "1")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - required installed ownership and mode: /usr/share/applications/omarchy-kids-browser-filter-demo.desktop\n"
    );
}

// Production mutation caught: a same-shaped file supplied by another package is not evidence that
// the reviewed demo package installed and owns the required asset.
#[test]
fn guest_acceptance_rejects_a_required_path_owned_by_another_package() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_PATH_OWNER", "other-package")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - required path is owned by omarchy-kids-browser-filter-demo: /usr/bin/omarchy-kids-browser-filter-demo\n"
    );
}

// Production mutation caught: checking only the expected positive paths permits the package to
// install system integration that would turn the controlled demo into a managed/default browser.
#[test]
fn guest_acceptance_rejects_every_unexpected_package_owned_non_directory_path() {
    let cases = [
        "/usr/lib/systemd/system/omarchy-kids-browser-filter-demo.service",
        "/etc/xdg/autostart/omarchy-kids-browser-filter-demo.desktop",
        "/etc/chromium/policies/managed/omarchy-kids-browser-filter-demo.json",
        "/usr/share/mime/packages/omarchy-kids-browser-filter-demo.xml",
        "/usr/share/omarchy-kids-browser-filter-demo/unexpected.dat",
    ];

    for unexpected in cases {
        let fixture = GuestAcceptanceFixture::new();
        write_file(
            &GuestAcceptanceFixture::rooted(&fixture.install_root, unexpected),
            "unexpected package integration\n",
        );
        let mut package_paths = fs::read_to_string(&fixture.package_paths).unwrap();
        package_paths.push_str(unexpected);
        package_paths.push('\n');
        write_file(&fixture.package_paths, &package_paths);

        let output = fixture.run(&[]);

        assert!(
            !output.status.success(),
            "unexpected path passed: {unexpected}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("not ok - package contains only expected non-directory paths: {unexpected}\n")
        );
    }
}

// Production mutation caught: enumerating only regular files hides an unexpected symlink in the
// package-owned extension directory and no longer proves that it has exactly two direct children.
#[test]
fn guest_acceptance_rejects_an_unexpected_extension_symlink() {
    let fixture = GuestAcceptanceFixture::new();
    let extension = GuestAcceptanceFixture::rooted(
        &fixture.install_root,
        "/usr/share/omarchy-kids-browser-filter-demo/browser-extension",
    );
    std::os::unix::fs::symlink("cover.css", extension.join("unexpected-link")).unwrap();

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser extension contains exactly manifest.json and cover.css\n"
    );
}

// Production mutation caught: enumerating only regular files hides an unexpected subdirectory in
// the package-owned extension directory and no longer proves the direct-child boundary.
#[test]
fn guest_acceptance_rejects_an_unexpected_extension_directory() {
    let fixture = GuestAcceptanceFixture::new();
    let extension = GuestAcceptanceFixture::rooted(
        &fixture.install_root,
        "/usr/share/omarchy-kids-browser-filter-demo/browser-extension",
    );
    fs::create_dir(extension.join("unexpected-directory")).unwrap();

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser extension contains exactly manifest.json and cover.css\n"
    );
}

// Production mutation caught: `-f` follows symlinks, so checking names and metadata through a fake
// package database can accept an exact-name link in place of a package-owned regular extension file.
#[test]
fn guest_acceptance_requires_the_named_extension_entries_to_be_regular_files() {
    let manifest = "/usr/share/omarchy-kids-browser-filter-demo/browser-extension/manifest.json";
    let fixture = GuestAcceptanceFixture::with_missing_path(Some(manifest));
    let manifest_path = GuestAcceptanceFixture::rooted(&fixture.install_root, manifest);
    std::os::unix::fs::symlink("cover.css", manifest_path).unwrap();

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("not ok - required installed path exists: {manifest}\n")
    );
}

// Production mutation caught: accepting different model bytes severs the installed proof from the
// reviewed detector identity and package preflight.
#[test]
fn guest_acceptance_rejects_the_wrong_installed_model_hash() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_MODEL_HASH", "wrong-model-hash")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - installed model SHA-256 matches the pinned model\n"
    );
}

// Production mutation caught: omitting the real launcher, headed-client observation, artifact
// capture, or cleanup can otherwise make installed file checks look like an end-to-end proof.
#[test]
fn guest_acceptance_proves_the_controlled_headed_fixture_and_cleanup() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[]);

    assert!(
        output.status.success(),
        "guest acceptance failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(fixture.artifacts.join("browser-filter-summary.json")).unwrap(),
        fs::read_to_string(&fixture.summary).unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.artifacts.join("browser-filter-metrics.jsonl")).unwrap(),
        fs::read_to_string(&fixture.metrics).unwrap()
    );
    assert_eq!(fs::read_to_string(&fixture.process_state).unwrap(), "111\n");
    assert_eq!(fs::read_dir(&fixture.profile_root).unwrap().count(), 0);
}

// Production mutation caught: masking the real background status accepts a launcher that emitted
// plausible artifacts and then reported failure.
#[test]
fn guest_acceptance_propagates_a_failed_launcher_status() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_LAUNCHER_STATUS", "23")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - controlled demo launcher exits successfully\n"
    );
}

// Production mutation caught: treating arbitrary nonempty launcher output as proof accepts a
// truncated or otherwise invalid summary artifact.
#[test]
fn guest_acceptance_rejects_invalid_summary_json() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(&fixture.summary, "not-json\n");

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: weakening any exact controlled-fixture count accepts a plausible
// but incorrect run summary.
#[test]
fn guest_acceptance_rejects_an_incorrect_summary_contract() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen("\"intercepted\":17", "\"intercepted\":16", 1),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: jq orders a missing field as null, so an upper-bound-only predicate
// accepts a summary that never reports reveal latency.
#[test]
fn guest_acceptance_rejects_a_missing_reveal_latency() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen("\"reveal_latency_millis\":500,", "", 1),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: jq considers null lower than numbers, so null satisfies only a
// maximum comparison without proving a measured numeric latency.
#[test]
fn guest_acceptance_rejects_a_null_reveal_latency() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"reveal_latency_millis\":500",
            "\"reveal_latency_millis\":null",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: a numeric-looking JSON string is not a measured numeric latency and
// must not gain acceptance through later predicate rewrites or coercion.
#[test]
fn guest_acceptance_rejects_a_string_reveal_latency() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"reveal_latency_millis\":500",
            "\"reveal_latency_millis\":\"500\"",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: a negative number satisfies an upper bound but cannot be a real
// elapsed reveal latency.
#[test]
fn guest_acceptance_rejects_a_negative_reveal_latency() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"reveal_latency_millis\":500",
            "\"reveal_latency_millis\":-1",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: requested hold duration alone does not prove that the opaque cover
// actually remained in place for the full reviewed interval.
#[test]
fn guest_acceptance_rejects_a_short_actual_cover_hold() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"actual_hold_millis\":1500",
            "\"actual_hold_millis\":1499",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: a missing measured hold must not be accepted as if it were zero or
// inferred from the requested duration.
#[test]
fn guest_acceptance_rejects_a_missing_actual_cover_hold() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen("\"actual_hold_millis\":1500,", "", 1),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: a numeric-looking string is not measured duration evidence.
#[test]
fn guest_acceptance_rejects_a_non_numeric_actual_cover_hold() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"actual_hold_millis\":1500",
            "\"actual_hold_millis\":\"1500\"",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: fewer than three hold screenshots under-samples the no-flash
// interval even when the first and last reported pixels happen to be covered.
#[test]
fn guest_acceptance_rejects_too_few_cover_hold_screenshots() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(
        &fixture.summary,
        &valid_guest_summary().replacen(
            "\"hold_screenshot_count\":3",
            "\"hold_screenshot_count\":2",
            1,
        ),
    );

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser summary proves the controlled fixture contract\n"
    );
}

// Production mutation caught: accepting a partial JSONL stream loses one or more response-stage
// records while retaining individually valid metric objects.
#[test]
fn guest_acceptance_rejects_an_incorrect_metric_count() {
    let fixture = GuestAcceptanceFixture::new();
    let short_metrics = valid_guest_metrics()
        .lines()
        .take(33)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    write_file(&fixture.metrics, &short_metrics);

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser metrics contain exactly 34 records\n"
    );
}

// Production mutation caught: validating only JSON syntax accepts URL/path/image/identity/error
// fields that violate the metric privacy boundary.
#[test]
fn guest_acceptance_rejects_non_privacy_safe_metric_keys() {
    let fixture = GuestAcceptanceFixture::new();
    let unsafe_metrics = valid_guest_metrics().replacen(
        "\"verdict\":\"allow\"}",
        "\"verdict\":\"allow\",\"url\":\"http://fixture.invalid/image.png\"}",
        1,
    );
    write_file(&fixture.metrics, &unsafe_metrics);

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - browser metrics contain only privacy-safe keys\n"
    );
}

// Production mutation caught: accepting an existing stale Chromium window as the launched client
// does not prove that the dedicated command started a headed browser.
#[test]
fn guest_acceptance_requires_a_new_headed_chromium_client() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_OBSERVE_CLIENT", "0")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - headed Chromium client is observed for this launch\n"
    );
}

// Production mutation caught: trusting the launcher's summary without checking the filesystem can
// leave its disposable browser profile and browsing state behind.
#[test]
fn guest_acceptance_rejects_a_leaked_disposable_profile() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_LEAK_PROFILE", "1")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - disposable Chromium profile is removed\n"
    );
}

// Production mutation caught: checking only the launcher PID or profile cleanup can miss a
// Chromium child that survived after the dedicated command returned.
#[test]
fn guest_acceptance_rejects_a_leaked_launched_chromium_process() {
    let fixture = GuestAcceptanceFixture::new();

    let output = fixture.run(&[("FAKE_LEAK_PROCESS", "1")]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - launched Chromium process is gone\n"
    );
}

// Production mutation caught: succeeding without the outer harness's tee output permits artifact
// collection to omit the required execution log.
#[test]
fn guest_acceptance_requires_a_nonempty_external_acceptance_log() {
    let fixture = GuestAcceptanceFixture::new();
    write_file(&fixture.artifacts.join("acceptance.log"), "");

    let output = fixture.run(&[]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "not ok - external acceptance log is nonempty\n"
    );
}

// Production mutations caught: skipping session discovery, changing the 420-second bound, running
// a different child, or collapsing its status all break the external acceptance hook contract.
#[test]
fn acceptance_runner_discovers_the_session_and_preserves_child_status() {
    let fixture = tempfile::tempdir().unwrap();
    let runtime = fixture.path().join("runtime");
    let signature = "fixture-signature";
    fs::create_dir_all(runtime.join("hypr").join(signature)).unwrap();
    write_file(&runtime.join("wayland-9"), "socket fixture");
    let artifacts = fixture.path().join("artifacts with spaces");
    let fake_bin = fixture.path().join("fake bin");
    let log = fixture.path().join("runner log");
    fs::create_dir_all(&fake_bin).unwrap();
    write_bash_executable(
        &fake_bin.join("hyprctl"),
        "[[ $1 == -j && $2 == monitors ]] || exit 64\nprintf '[]\\n'\n",
    );
    write_bash_executable(
        &fake_bin.join("timeout"),
        r#"printf 'ARG=%s\n' "$@" >"$RUNNER_LOG"
printf 'XDG=%s\nDBUS=%s\nHYPR=%s\nWAYLAND=%s\nARTIFACTS=%s\n' \
  "$XDG_RUNTIME_DIR" "$DBUS_SESSION_BUS_ADDRESS" "$HYPRLAND_INSTANCE_SIGNATURE" \
  "$WAYLAND_DISPLAY" "$OMARCHY_ACCEPTANCE_DIR" >>"$RUNNER_LOG"
exit 37
"#,
    );
    let mut command = Command::new("bash");
    command
        .arg(repository_root().join("test/acceptance"))
        .current_dir(repository_root())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("DISPLAY", ":0")
        .env("LANG", "C.UTF-8")
        .env("OMARCHY_ACCEPTANCE_DIR", &artifacts)
        .env("RUNNER_LOG", &log)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env_remove("WAYLAND_DISPLAY");
    prepend_path(&mut command, &fake_bin);

    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(37));
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        format!(
            "ARG=420\nARG=bash\nARG={}\nXDG={}\nDBUS=unix:path={}/bus\nHYPR={}\nWAYLAND=wayland-9\nARTIFACTS={}\n",
            repository_root()
                .join("test/acceptance.d/adult-content-filter-test.sh")
                .display(),
            runtime.display(),
            runtime.display(),
            signature,
            artifacts.display()
        )
    );
    assert!(artifacts.is_dir());
}
