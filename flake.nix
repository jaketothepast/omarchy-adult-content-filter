{
  description = "Local-only Omarchy Kids managed-browser filter experiment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/9387b3fcc0c23c86661636da63faabad4235a0a6";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      model = pkgs.fetchurl {
        url = "https://raw.githubusercontent.com/notAI-tech/NudeNet/6ccc81c6c305cccfd46d92b414f8a5c0a816574d/nudenet/320n.onnx";
        hash = "sha256-wV2Cc62tLQqS8BTMaastbDEaBnd6VVRfLE60b1GRHw8=";
      };
      environment = {
        ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/libonnxruntime.so.${pkgs.onnxruntime.version}";
        NUDENET_MODEL_PATH = model;
        CHROMIUM_BIN = "${pkgs.chromium}/bin/chromium";
        OMARCHY_KIDS_EXTENSION_DIR = ./browser-extension;
      };
      package = pkgs.rustPlatform.buildRustPackage {
        pname = "omarchy-kids-browser-filter";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.makeWrapper ];
        nativeCheckInputs = [ pkgs.clippy pkgs.rustfmt ];
        checkPhase = ''
          runHook preCheck
          export ORT_DYLIB_PATH=${environment.ORT_DYLIB_PATH}
          export NUDENET_MODEL_PATH=${environment.NUDENET_MODEL_PATH}
          cargo fmt --check
          cargo clippy --workspace --all-targets -- -D warnings
          cargo test --workspace
          runHook postCheck
        '';
        postFixup = ''
          wrapProgram $out/bin/omarchy-kids-browser-filter \
            --set ORT_DYLIB_PATH ${environment.ORT_DYLIB_PATH} \
            --set NUDENET_MODEL_PATH ${environment.NUDENET_MODEL_PATH} \
            --set CHROMIUM_BIN ${environment.CHROMIUM_BIN} \
            --set OMARCHY_KIDS_EXTENSION_DIR ${environment.OMARCHY_KIDS_EXTENSION_DIR}
        '';
      };
      check = pkgs.writeShellApplication {
        name = "omarchy-kids-browser-filter-check";
        runtimeInputs = [ pkgs.cargo pkgs.clippy pkgs.rustfmt ];
        text = ''
          export ORT_DYLIB_PATH=${environment.ORT_DYLIB_PATH}
          export NUDENET_MODEL_PATH=${environment.NUDENET_MODEL_PATH}
          cargo fmt --check
          cargo clippy --workspace --all-targets -- -D warnings
          cargo test --workspace
        '';
      };
      app = command: {
        type = "app";
        program = "${pkgs.writeShellScript "omarchy-kids-browser-filter-${command}" ''
          exec ${package}/bin/omarchy-kids-browser-filter ${command} "$@"
        ''}";
      };
    in
    {
      devShells.${system}.default = pkgs.mkShell (environment // {
        packages = [
          pkgs.cargo
          pkgs.clippy
          pkgs.rust-analyzer
          pkgs.rustc
          pkgs.rustfmt
          pkgs.pkg-config
          pkgs.onnxruntime
          pkgs.chromium
        ];
      });
      packages.${system}.default = package;
      checks.${system}.default = package;
      formatter.${system} = pkgs.nixfmt;
      apps.${system} = {
        doctor = app "doctor";
        infer = app "infer";
        bench = app "bench";
        run = app "run";
        check = {
          type = "app";
          program = "${check}/bin/omarchy-kids-browser-filter-check";
        };
      };
    };
}
