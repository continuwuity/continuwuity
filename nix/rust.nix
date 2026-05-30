{ inputs, ... }:
{
  perSystem =
    {
      system,
      lib,
      pkgs,
      ...
    }:
    {
      packages =
        let
          fnx = inputs.fenix.packages.${system};

          stable-toolchain = fnx.fromToolchainFile {
            file = inputs.self + "/rust-toolchain.toml";

            # See also `rust-toolchain.toml`
            sha256 = "sha256-mvUGEOHYJpn3ikC5hckneuGixaC+yGrkMM/liDIDgoU=";
          };
        in
        {
          inherit stable-toolchain;

          dev-toolchain = fnx.combine [
            stable-toolchain
            # use the nightly rustfmt because we use nightly features
            fnx.complete.rustfmt
          ];
        };
    };
}
