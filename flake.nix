{
  description = "eve sekondo";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };

  outputs = { self, nixpkgs, nixgl, flake-parts, rust-overlay, ... }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.flake-parts.flakeModules.easyOverlay
      ];

      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];

      perSystem = { config, self', inputs', pkgs, system, lib, ... }:
        let
          overlays = [ (import rust-overlay) nixgl.overlay ];

          pkgs = import nixpkgs {
            inherit system overlays;
          };

          rustDevEnvironment = inputs'.rust-overlay.packages.rust.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" ];
          };

          shellDeps = with pkgs; [
            rustDevEnvironment

            gh
            nil
            nixpkgs-fmt
            nodejs_24
            markdownlint-cli
            prettier

            sqlx-cli
            mkcert

            podman
            podman-compose

            postgresql_17
            openssl
          ];

          shell = pkgs.mkShell {
            nativeBuildInputs = shellDeps
            ++ lib.optionals (!pkgs.stdenv.isDarwin) [
              pkgs.vscode-extensions.vadimcn.vscode-lldb
            ];
            shellHook = ''
              export PATH=~/.cargo/bin:$PATH

              ${ if (!pkgs.stdenv.isDarwin) then
                  "export PATH=${pkgs.vscode-extensions.vadimcn.vscode-lldb}/share/vscode/extensions/vadimcn.vscode-lldb/adapter:$PATH"
                else
                  ""
              }
            '';

            RUST_BACKTRACE = "full";

            # NixOS can't run Playwright's downloaded browsers; use the nixpkgs build.
            # `@playwright/test` in e2e/package.json must match playwright-driver's version.
            PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
            PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
          };
        in
        {
          devShells.default = shell;
        };
    };
}
