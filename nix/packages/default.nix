{
  self,
  ...
}:
{
  perSystem =
    {
      self',
      pkgs,
      craneLib,
      ...
    }:
    {
      packages = {
        rocksdb = pkgs.callPackage ./rocksdb.nix { };
        default = pkgs.callPackage ./continuwuity.nix {
          inherit self craneLib;
          # extra features via `cargoExtraArgs`
          cargoExtraArgs = "-F http3";
          # extra RUSTFLAGS via `rustflags`
          # the stuff below is required for http3
          rustflags = "--cfg reqwest_unstable";
        };
        # users may also override this with other cargo profiles to build for other feature sets
        #
        # other examples include:
        #
        # - release-high-perf
        max-perf = self'.packages.default.override {
          profile = "release-max-perf";
        };
      };
    };
}
