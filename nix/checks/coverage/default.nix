{
  perSystem =
    {
      pkgs,
      craneLib,
      cargoLlvmCov,
      commonArgs,
      ...
    }:
    {
      checks.coverage = craneLib.mkCargoDerivation (
        commonArgs
        // {
          pnameSuffix = "-coverage";
          cargoArtifacts = null;
          CARGO_PROFILE = "dev";
          nativeBuildInputs = [
            cargoLlvmCov
            pkgs.cargo-nextest
            pkgs.jq
            pkgs.lcov
            pkgs.python3Packages.lcov-cobertura
          ];
          buildPhaseCargoCommand = "bash ${./collect.sh} $out";
          doCheck = false;
          doInstallCargoArtifacts = false;
          installPhase = "true";
        }
      );
    };
}
