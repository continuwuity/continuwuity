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
          inherit (self'.packages) rocksdb;
          # extra features via `cargoExtraArgs`
          cargoExtraArgs = "-F http3";
          # extra RUSTFLAGS via `rustflags`
          # the stuff below is required for http3
          rustflags = "--cfg reqwest_unstable";
        };
        # users may also override this with other cargo profiles to build for other feature sets
        # for features configuration see `default` package which enables http3 by default

        # example: different compilation profile and different target_cpu
        max-perf-haswell = self'.packages.default.override {
          # compiles explicitly for haswell arch cpus
          target_cpu = "haswell";
          # compiles slower but with more thorough optimizations
          profile = "release-max-perf";
        };
      };
    };
}
