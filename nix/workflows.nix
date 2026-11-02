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
            # actionlint 1.7.12 predates GitHub's code-quality permission.
            actionlint -ignore 'unknown permission scope "code-quality"' \
              "$src/.github/workflows/"*.yml
            mkdir -p "$out"
          '';
    };
}
