{ inputs, ... }:
{
  perSystem =
    {
      lib,
      inputs',
      pkgs,
      ...
    }:
    let
      mkToolchain =
        target:
        target.fromToolchainName {
          name = (lib.importTOML "${inputs.self}/rust-toolchain.toml").toolchain.channel;
          sha256 = "sha256-+xUr6Al1wZSqX8aEsq7DpuFOK4Ve/i+qTl+K5uWQ9GM=";
        };
    in
    {
      _module.args = { inherit mkToolchain; };

      packages =
        let
          fnx = inputs'.fenix.packages;
          stable-toolchain = (mkToolchain fnx).toolchain;
        in
        {
          inherit stable-toolchain;

          dev-toolchain = fnx.combine [
            # use the nightly rustfmt because we use nightly features
            fnx.complete.rustfmt
            stable-toolchain
          ];
        };
    };
}
