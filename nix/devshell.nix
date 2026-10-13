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
        packages = [
          pkgs.jujutsu
          config.treefmt.build.wrapper
        ];
      };
    };
}
