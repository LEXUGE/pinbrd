{
  description = "A prototype for productive knowledge management";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    utils.url = "github:numtide/flake-utils";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      utils,
      git-hooks,
      ...
    }:
    with nixpkgs.lib;
    let
      pkgsWithRust =
        system:
        import nixpkgs {
          system = "${system}";
          overlays = [ rust-overlay.overlays.default ];
        };
      pkgSet = system: {
        pinbrd =
          with (pkgsWithRust system);
          (makeRustPlatform {
            cargo = rust-bin.stable.latest.default;
            rustc = rust-bin.stable.latest.default;
          }).buildRustPackage
            {
              name = "pinbrd";
              version = "git";
              src = lib.cleanSource ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
            };
      };
    in
    utils.lib.eachSystem
      (with utils.lib.system; [
        x86_64-linux
      ])
      (system: rec {
        packages = (pkgSet system);

        checks = {
          pre-commit-check = git-hooks.lib.${system}.run {
            src = ./.;
            hooks = {
              nixfmt-rfc-style.enable = true;
              rustfmt.enable = true;
            };
          };
        };

        apps = rec {
          default = pinbrd;
          pinbrd = (
            utils.lib.mkApp {
              drv = packages."pinbrd";
            }
          );
        };

        devShells.default =
          with (pkgsWithRust system);
          mkShell {
            inherit (self.checks.${system}.pre-commit-check) shellHook;
            nativeBuildInputs = [
              # # write rustfmt first to ensure we are using nightly rustfmt
              # rust-bin.nightly."2025-01-01".rustfmt
              rust-bin.stable.latest.default
              rust-bin.stable.latest.rust-src
              rust-analyzer

              binutils-unwrapped
              cargo-cache
              cargo-outdated
            ];
          };
      })
    // {
      overlays.default = final: prev: {
        pinbrd = recurseIntoAttrs (pkgSet prev.pkgs.system);
      };
    };
}
