{
  pkgs,
  package,
  check,
  environment,
  source,
}:

let
  inherit (pkgs) lib;

  mkApp =
    {
      name,
      description,
      command,
      runtimeInputs ? [ ],
      runtimeEnvironment ? { },
    }:
    let
      application = pkgs.writeShellApplication {
        name = "omarchy-kids-${name}";
        inherit runtimeInputs;
        text = ''
          ${lib.concatStringsSep "\n" (
            lib.mapAttrsToList (
              variable: value: "export ${variable}=${lib.escapeShellArg (toString value)}"
            ) runtimeEnvironment
          )}
          exec ${command} "$@"
        '';
      };
    in
    {
      type = "app";
      program = lib.getExe application;
      meta = { inherit description; };
    };

  scriptCommand = name: "${pkgs.bash}/bin/bash ${source}/scripts/${name}";

  firmwareEnvironment = {
    OMARCHY_VM_OVMF_CODE = "${pkgs.OVMF.fd}/FV/OVMF_CODE.fd";
    OMARCHY_VM_OVMF_VARS_TEMPLATE = "${pkgs.OVMF.fd}/FV/OVMF_VARS.fd";
  };

  isoUnitInputs = with pkgs; [
    coreutils
    diffutils
    file
    findutils
    gawk
    git
    gnugrep
    gnused
    python3Minimal
  ];

  isoVmInputs = with pkgs; [
    coreutils
    dosfstools
    findutils
    gawk
    gnugrep
    gnutar
    imagemagick
    jq
    mtools
    openssh
    procps
    python3
    qemu
    socat
    tesseract
    util-linux
  ];
in
{
  infer = mkApp {
    name = "infer";
    description = "Run bounded local ONNX inference for one image";
    command = "${package}/bin/omarchy-kids-browser-filter infer";
  };

  bench = mkApp {
    name = "bench";
    description = "Benchmark the pinned local ONNX image detector";
    command = "${package}/bin/omarchy-kids-browser-filter bench";
  };

  run = mkApp {
    name = "run";
    description = "Run the controlled headed Chromium interception experiment";
    command = "${package}/bin/omarchy-kids-browser-filter run";
  };

  browse = mkApp {
    name = "browse";
    description = "Run the persistent single-tab managed Chromium prototype";
    command = "${package}/bin/omarchy-kids-browser-filter browse";
  };

  check = mkApp {
    name = "check";
    description = "Run formatting, lint, and workspace tests";
    command = "${check}/bin/omarchy-kids-browser-filter-check";
  };

  doctor = mkApp {
    name = "doctor";
    description = "Check all host prerequisites for the local Omarchy ISO workflow";
    command = scriptCommand "doctor";
    runtimeInputs = with pkgs; [
      coreutils
      docker-client
      gawk
      git
    ];
    runtimeEnvironment = environment // firmwareEnvironment;
  };

  iso-unit = mkApp {
    name = "iso-unit";
    description = "Run the Omarchy ISO repository's VM-free unit test suite";
    command = scriptCommand "iso-unit";
    runtimeInputs = isoUnitInputs;
  };

  iso-build = mkApp {
    name = "iso-build";
    description = "Build an unattended Omarchy ISO from the local sibling checkouts";
    command = scriptCommand "iso-build";
    runtimeInputs = isoUnitInputs ++ (with pkgs; [ docker-client ]);
  };

  adult-filter-iso-build = mkApp {
    name = "adult-filter-iso-build";
    description = "Build an Omarchy ISO containing the opt-in adult content filter";
    command = scriptCommand "adult-filter-iso-build";
    runtimeInputs = isoUnitInputs ++ (with pkgs; [ docker-client ]);
    runtimeEnvironment.OMARCHY_ADULT_FILTER_PACKAGE_SOURCE = source;
  };

  iso-test = mkApp {
    name = "iso-test";
    description = "Run the Omarchy ISO acceptance harness with Nix-managed host tools";
    command = scriptCommand "iso-test";
    runtimeInputs = isoVmInputs;
    runtimeEnvironment = firmwareEnvironment;
  };

  adult-filter-iso-test = mkApp {
    name = "adult-filter-iso-test";
    description = "Validate the installed adult content filter in an Omarchy VM";
    command = scriptCommand "adult-filter-iso-test";
    runtimeInputs = isoVmInputs;
    runtimeEnvironment = firmwareEnvironment // {
      OMARCHY_ADULT_FILTER_PACKAGE_SOURCE = source;
    };
  };

  iso-integration = mkApp {
    name = "iso-integration";
    description = "Run the Omarchy ISO integration scenarios with Nix-managed host tools";
    command = scriptCommand "iso-integration";
    runtimeInputs = isoVmInputs ++ [ pkgs.openssl ];
    runtimeEnvironment = firmwareEnvironment;
  };
}
