{ inputs, ... }:
{
  # load the flake module from upstream
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { self', lib, ... }:
    {
      treefmt = {
        # repo root as project root
        projectRoot = inputs.self;

        # the formatters
        programs = {
          nixfmt.enable = true;
          typos = {
            enable = true;
            configFile = "${inputs.self}/.typos.toml";
          };
          taplo = {
            enable = true;
            settings = lib.importTOML "${inputs.self}/taplo.toml";
          };
        };

        settings.formatter.rustfmt = {
          command = "${lib.getExe' self'.packages.dev-toolchain "rustfmt"}";
          includes = [ "**/*.rs" ];
          options = [
            "--unstable-features"
            "--edition=2024"
            "--config-path=${inputs.self}/rustfmt.toml"
          ];
        };
      };
    };
}
