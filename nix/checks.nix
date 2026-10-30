{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      craneLib,
      commonArgs,
      cargoArtifacts,
      rustToolchain,
      ...
    }:
    let
      checkArgs = commonArgs // {
        inherit cargoArtifacts;
      };
      cargoVendorDir = craneLib.vendorCargoDeps commonArgs;
    in
    {
      checks = {
        doc = craneLib.cargoDoc (
          checkArgs
          // {
            cargoDocExtraArgs = "--workspace --no-deps --all-features";
            RUSTDOCFLAGS = "-D warnings";
          }
        );
        clippy = craneLib.cargoClippy (
          checkArgs
          // {
            cargoClippyExtraArgs = "--workspace --all-targets --all-features -- -D warnings";
          }
        );
        tests = craneLib.cargoTest (
          checkArgs
          // {
            cargoTestExtraArgs = "--workspace --all-targets --all-features";
          }
        );
        doctests = craneLib.cargoDocTest (
          checkArgs
          // {
            cargoTestExtraArgs = "--workspace --all-features";
          }
        );
        cargo-deny = craneLib.cargoDeny (
          commonArgs
          // {
            cargoDenyExtraArgs = "--config ${../deny.toml}";
          }
        );
        # Crane's cargoDeny covers bans, licences and sources. Use the pinned
        # RustSec snapshot for advisories, without a database fetch at build time.
        advisories = craneLib.cargoAudit {
          src = commonArgs.src;
          advisory-db = inputs.advisory-db;
          inherit (craneLib.crateNameFromCargoToml { cargoToml = ../Cargo.toml; }) pname version;
        };
        cargo-shear =
          pkgs.runCommand "hartig-cargo-shear"
            {
              inherit cargoVendorDir;
              inherit (commonArgs) src;
              nativeBuildInputs = [
                rustToolchain
                pkgs.cargo-shear
              ];
            }
            ''
              export CARGO_HOME="$TMPDIR/cargo-home"
              export CARGO_NET_OFFLINE=true
              mkdir -p "$CARGO_HOME"
              cp "$cargoVendorDir/config.toml" "$CARGO_HOME/config.toml"
              cd "$src"
              cargo shear
              mkdir -p "$out"
            '';
      };
    };
}
