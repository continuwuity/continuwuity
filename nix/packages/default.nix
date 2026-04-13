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
        default = pkgs.callPackage ./continuwuity.nix { inherit self craneLib; };
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
