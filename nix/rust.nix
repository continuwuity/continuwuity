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
          sha256 = "sha256-p8h3Sl/YRByZfZTAKXdsvF6xEenXKrXSVvpphmZENH4=";
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
