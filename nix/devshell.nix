{
  perSystem =
    {
      config,
      pkgs,
      craneLib,
      ...
    }:
    {
      devShells.default = craneLib.devShell {
        checks = config.checks;
        packages = [
          config.packages.validate-commits
          pkgs.cargo-sort
          pkgs.jujutsu
          pkgs.ripgrep
          config.treefmt.build.wrapper
        ];
      };
    };
}
