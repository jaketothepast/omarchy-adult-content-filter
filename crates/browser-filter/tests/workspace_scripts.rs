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
