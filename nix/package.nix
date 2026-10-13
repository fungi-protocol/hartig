{ inputs, ... }:
{
  perSystem =
    { craneLib, ... }:
    let
      commonArgs = {
        src = craneLib.cleanCargoSource inputs.self;
        strictDeps = true;
      };
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      hartig = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
          doCheck = false;
        }
      );
    in
    {
      _module.args = {
        inherit
          commonArgs
          cargoArtifacts
          ;
      };
      packages.default = hartig;
      checks.build = hartig;
    };
}
