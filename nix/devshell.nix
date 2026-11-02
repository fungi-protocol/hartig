{
  perSystem =
    {
      config,
      pkgs,
      craneLib,
      cargoLlvmCov,
      ...
    }:
    {
      devShells.default = craneLib.devShell {
        checks = config.checks;
        packages = [
          config.packages.validate-commits
          cargoLlvmCov
          pkgs.cargo-nextest
          pkgs.cargo-sort
          pkgs.jujutsu
          pkgs.ripgrep
          config.treefmt.build.wrapper
        ];
      };
    };
}
