{ inputs, ... }:
{
  perSystem =
    {
      system,
      lib,
      ...
    }:
    {
      packages =
        let
          fnx = inputs.fenix.packages.${system};

          stable = fnx.fromToolchainFile {
            file = inputs.self + "/rust-toolchain.toml";

            # See also `rust-toolchain.toml`
            sha256 = "sha256-sqSWJDUxc+zaz1nBWMAJKTAGBuGWP25GCftIOlCEAtA=";
          };
        in
        {
          # used for building nix stuff (doesn't include rustfmt overhead)
          build-toolchain = stable;
          # used for dev shells
          dev-toolchain = fnx.combine [
            stable
            # use the nightly rustfmt because we use nightly features
            fnx.complete.rustfmt
          ];
        };
    };
}
