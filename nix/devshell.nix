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
          pkgs.cargo-sort
          pkgs.jujutsu
          pkgs.ripgrep
          config.treefmt.build.wrapper
        ];
      };
    };
}
