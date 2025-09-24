{ inputs, ... }:
{
  perSystem =
    {
      self',
      lib,
      pkgs,
      ...
    }:
    let
      uwulib = inputs.self.uwulib.init pkgs;
    in
    {
      packages =
        lib.pipe
          [
            # this is the default variant
            {
              variantName = "default";
              commonAttrsArgs.profile = "release";
              rocksdb = self'.packages.rocksdb;
              features = { };
            }
            # this is the variant with all features enabled (liburing + jemalloc)
            {
              variantName = "all-features";
              commonAttrsArgs.profile = "release";
              rocksdb = self'.packages.rocksdb.override {
                enableJemalloc = true;
                enableLiburing = true;
              };
              features = {
                enabledFeatures = "all";
                disabledFeatures = uwulib.features.defaultDisabledFeatures ++ [ "bindgen-static" ];
              };
            }
          ]
          [
            (builtins.map (cfg: rec {
              deps = {
                name = "continuwuity-${cfg.variantName}-deps";
                value = uwulib.build.buildDeps {
                  features = uwulib.features.calcFeatures cfg.features;
                  inherit (cfg) commonAttrsArgs rocksdb;
                };
              };
              bin = {
                name = "continuwuity-${cfg.variantName}-bin";
                value = uwulib.build.buildPackage {
                  deps = self'.packages.${deps.name};
                  features = uwulib.features.calcFeatures cfg.features;
                  inherit (cfg) commonAttrsArgs rocksdb;
                };
              };
            }))
            (builtins.concatMap builtins.attrValues)
            builtins.listToAttrs
          ];
    };
}
