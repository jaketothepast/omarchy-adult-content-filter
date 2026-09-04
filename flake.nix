{
  description = "Omarchy Adult Content Filter managed-browser experiment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/9387b3fcc0c23c86661636da63faabad4235a0a6";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      model = pkgs.fetchurl {
        url = "https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/nudenet/320n.onnx";
        hash = "sha256-wV2Cc62tLQqS8BTMaastbDEaBnd6VVRfLE60b1GRHw8=";
      };
      adultDomains = pkgs.fetchurl {
        url = "https://raw.githubusercontent.com/StevenBlack/hosts/2bb49d741a2c9b922b0ed59be6c28ce543bed81b/alternates/porn-only/hosts";
        hash = "sha256-pRLCgV/mEvpO7ox7Hi2rF/ejkBZInqs+O7zEB+209RQ=";
      };
      environment = {
        ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/libonnxruntime.so.${pkgs.onnxruntime.version}";
        NUDENET_MODEL_PATH = model;
        OMARCHY_KIDS_BLOCKLIST_PATH = adultDomains;
        CHROMIUM_BIN = "${pkgs.chromium}/bin/chromium";
        OMARCHY_KIDS_EXTENSION_DIR = ./browser-extension;
      };
      package = pkgs.rustPlatform.buildRustPackage {
        pname = "omarchy-kids-browser-filter";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.makeWrapper ];
        nativeCheckInputs = [
          pkgs.clippy
          pkgs.git
          pkgs.jq
          pkgs.rustfmt
        ];
        checkPhase = ''
          runHook preCheck
          export ORT_DYLIB_PATH=${environment.ORT_DYLIB_PATH}
          export NUDENET_MODEL_PATH=${environment.NUDENET_MODEL_PATH}
          export OMARCHY_KIDS_BLOCKLIST_PATH=${environment.OMARCHY_KIDS_BLOCKLIST_PATH}
          cargo fmt --check
          cargo clippy --workspace --all-targets -- -D warnings
          cargo test --workspace
          runHook postCheck
        '';
        postFixup = ''
          wrapProgram $out/bin/omarchy-kids-browser-filter \
            --set ORT_DYLIB_PATH ${environment.ORT_DYLIB_PATH} \
            --set NUDENET_MODEL_PATH ${environment.NUDENET_MODEL_PATH} \
            --set OMARCHY_KIDS_BLOCKLIST_PATH ${environment.OMARCHY_KIDS_BLOCKLIST_PATH} \
            --set CHROMIUM_BIN ${environment.CHROMIUM_BIN} \
            --set OMARCHY_KIDS_EXTENSION_DIR ${environment.OMARCHY_KIDS_EXTENSION_DIR}
        '';
      };
      check = pkgs.writeShellApplication {
        name = "omarchy-kids-browser-filter-check";
        runtimeInputs = [
          pkgs.cargo
          pkgs.clippy
          pkgs.git
          pkgs.jq
          pkgs.rustfmt
        ];
        text = ''
          export ORT_DYLIB_PATH=${environment.ORT_DYLIB_PATH}
          export NUDENET_MODEL_PATH=${environment.NUDENET_MODEL_PATH}
          export OMARCHY_KIDS_BLOCKLIST_PATH=${environment.OMARCHY_KIDS_BLOCKLIST_PATH}
          cargo fmt --check
          cargo clippy --workspace --all-targets -- -D warnings
          cargo test --workspace
        '';
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell (
        environment
        // {
          packages = [
            pkgs.cargo
            pkgs.clippy
            pkgs.rust-analyzer
            pkgs.rustc
            pkgs.rustfmt
            pkgs.pkg-config
            pkgs.onnxruntime
            pkgs.chromium
            pkgs.jq
          ];
        }
      );
      packages.${system}.default = package;
      checks.${system}.default = package;
      formatter.${system} = pkgs.nixfmt;
      apps.${system} = import ./nix/apps.nix {
        inherit
          pkgs
          package
          check
          environment
          ;
        source = ./.;
      };
    };
}
