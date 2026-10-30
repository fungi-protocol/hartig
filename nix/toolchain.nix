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
          "rust-analyzer"
          "rust-src"
          "rustfmt"
        ];
      };
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
    in
    {
      _module.args = {
        inherit pkgs rustToolchain craneLib;
      };
    };
}
