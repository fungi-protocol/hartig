{
  description = "Hartig server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nix2container.url = "github:nlewo/nix2container";
  };

  outputs = { self, nixpkgs, nix2container }:
    let
      systems = [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" "x86_64-linux" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in {
      packages = eachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          hartig = pkgs.rustPlatform.buildRustPackage {
            pname = "hartig";
            version = "0.1.0";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
          };
        in {
          default = hartig;
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          container = nix2container.packages.${system}.nix2container.buildImage {
            name = "hartig";
            copyToRoot = [ hartig ];
            config = {
              Cmd = [ "${hartig}/bin/hartig" ];
              ExposedPorts = { "3000/tcp" = { }; };
            };
          };
        });

      devShells = eachSystem (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.mkShell {
            packages = [ pkgs.cargo pkgs.clippy pkgs.rustc pkgs.rustfmt ];
          };
        });
    };
}
