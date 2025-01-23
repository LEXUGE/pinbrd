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
        pinlab =
          with (pkgsWithRust system);
          (makeRustPlatform {
            cargo = rust-bin.stable.latest.default;
            rustc = rust-bin.stable.latest.default;
          }).buildRustPackage
            rec {
              name = "pinlab";
              version = "git";
              src = lib.cleanSource ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
              postFixup = ''
                patchelf $out/bin/${name} \
                  --add-rpath ${
                    lib.makeLibraryPath [
                      libGL
                      libxkbcommon
                      wayland
                    ]
                  }
              '';
              buildInputs = [
                libxkbcommon
                wayland
                libGL
                xorg.libXcursor
                xorg.libXi
                xorg.libX11
              ];
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
          default = pinlab;
          pinlab = (
            utils.lib.mkApp {
              drv = packages."pinlab";
            }
          );
        };

        devShells.default =
          with (pkgsWithRust system);
          mkShell {
            inherit (self.checks.${system}.pre-commit-check) shellHook;
            nativeBuildInputs = [
              # # write rustfmt first to ensure we are using nightly rustfmt
              rust-bin.nightly."2025-01-01".rustfmt
              rust-bin.stable.latest.default
              rust-bin.stable.latest.rust-src
              rust-analyzer

              binutils-unwrapped
              cargo-cache
              cargo-outdated
            ];
            LD_LIBRARY_PATH =
              with pkgs;
              lib.makeLibraryPath [
                libGL
                libxkbcommon
                wayland
                xorg.libXcursor
                xorg.libXi
                xorg.libX11
              ];
          };
      })
    // {
      overlays.default = final: prev: {
        pinlab = recurseIntoAttrs (pkgSet prev.pkgs.system);
      };
    };
}
