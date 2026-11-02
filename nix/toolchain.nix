{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    let
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ inputs.rust-overlay.overlays.default ];
      };
      # The locked rust-overlay input fixes the version selected by latest.
      rustToolchain = pkgs.rust-bin.stable.latest.minimal.override {
        extensions = [
          "clippy"
          "llvm-tools"
          "rust-analyzer"
          "rust-src"
          "rustfmt"
        ];
      };
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
      llvmBin = "${rustToolchain}/lib/rustlib/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/bin";
      # Keep the coverage tool's self-tests on a matching compiler/LLVM pair.
      cargoLlvmCov =
        (pkgs.cargo-llvm-cov.override {
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        }).overrideAttrs
          (previous: {
            env = previous.env // {
              LLVM_COV = "${llvmBin}/llvm-cov";
              LLVM_PROFDATA = "${llvmBin}/llvm-profdata";
            };
          });
    in
    {
      _module.args = {
        inherit
          pkgs
          rustToolchain
          craneLib
          cargoLlvmCov
          ;
      };
    };
}
