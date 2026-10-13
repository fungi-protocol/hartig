{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      checks.actionlint =
        pkgs.runCommand "hartig-actionlint"
          {
            src = inputs.self;
            nativeBuildInputs = [
              pkgs.actionlint
              pkgs.shellcheck
            ];
          }
          ''
            actionlint "$src/.github/workflows/"*.yml
            mkdir -p "$out"
          '';
    };
}
