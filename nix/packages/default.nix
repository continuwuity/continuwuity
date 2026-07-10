{
  inputs,
  self,
  ...
}:
{
  perSystem =
    {
      self',
      lib,
      pkgs,
      inputs',
      system,
      craneLib,
      mkToolchain,
      ...
    }:
    {
      packages =
        let
          mkPackages =
            pkgs:
            let
              fnx = inputs'.fenix.packages;

              isStatic = pkgs.stdenv.hostPlatform.isMusl;

              craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
                _:
                if isStatic then
                  fnx.combine [
                    self'.packages.stable-toolchain
                    (mkToolchain fnx.targets.${pkgs.stdenv.hostPlatform.config}).rust-std
                  ]
                else
                  self'.packages.stable-toolchain
              );

              # extra features via `cargoExtraArgs`
              cargoExtraArgs = "-F http3";

              default = pkgs.callPackage ./continuwuity.nix {
                inherit self craneLib;

                liburing = (if isStatic then pkgs.pkgsStatic else pkgs).liburing;
                rocksdb = if isStatic then null else self'.packages.rocksdb;

                inherit cargoExtraArgs;
                # extra RUSTFLAGS via `rustflags`
                # the stuff below is required for http3
                rustflags = "--cfg reqwest_unstable";
              };

              # users may also override this with other cargo profiles to build for other feature sets
              # for features configuration see `default` package which enables http3 by default

              max-perf = default.override {
                # compiles slower but with more thorough optimizations
                profile = "release-max-perf";
                cargoExtraArgs = "${cargoExtraArgs} -F release_max_log_level";
              };

              max-perf-haswell = max-perf.override {
                # compiles explicitly for haswell arch cpus
                target_cpu = "haswell";
              };
            in
            {
              inherit default max-perf max-perf-haswell;
            };
        in
        {
          rocksdb = pkgs.callPackage ./rocksdb.nix { };
        }
        // (mkPackages pkgs)
        // (lib.mapAttrs' (name: value: lib.nameValuePair "${name}-static-x86_64" value) (
          mkPackages (
            import inputs.nixpkgs {
              localSystem = system;
              crossSystem = "x86_64-unknown-linux-musl";
            }
          )
        ))
        // (lib.mapAttrs' (name: value: lib.nameValuePair "${name}-static-aarch64" value) (
          mkPackages (
            import inputs.nixpkgs {
              localSystem = system;
              crossSystem = "aarch64-unknown-linux-musl";
            }
          )
        ));
    };
}
