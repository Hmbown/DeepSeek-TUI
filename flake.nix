{
  description = "DeepSeek-TUI — terminal AI coding agent for DeepSeek V4";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # DeepSeek-TUI requires nightly for `if let` guards and edition 2024.
        rustToolchain = pkgs.rust-bin.nightly.latest.default;

        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];

        buildInputs = with pkgs; [
          dbus
          openssl
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.dbus.dev
        ];

      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "deepseek-tui";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version or "0.8.11";

          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          inherit nativeBuildInputs buildInputs;

          # Build both the CLI dispatcher and the TUI binary.
          cargoBuildFlags = [ "-p" "deepseek-cli" "-p" "deepseek-tui" ];

          # The nightly toolchain is declared in rust-toolchain.toml;
          # pass RUSTUP_TOOLCHAIN so rustPlatform picks it up correctly.
          RUSTUP_TOOLCHAIN = "nightly";

          meta = with pkgs.lib; {
            description = "Terminal AI coding agent for DeepSeek V4 with 1M-token context";
            homepage = "https://github.com/Hmbown/DeepSeek-TUI";
            license = licenses.mit;
            maintainers = [ ];
            platforms = platforms.unix;
            mainProgram = "deepseek-tui";
          };
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            rust-analyzer
            cargo-watch
            git
          ]);

          RUST_BACKTRACE = "1";
          shellHook = ''
            echo "DeepSeek-TUI dev shell ready. Run: cargo +nightly build"
          '';
        };
      });
}
