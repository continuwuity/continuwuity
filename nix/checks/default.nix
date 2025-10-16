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

      rocksdbAllFeatures = self'.packages.rocksdb.override {
        enableJemalloc = true;
        enableLiburing = true;
      };

      commonAttrs = (uwulib.build.commonAttrs { }) // {
        buildInputs = [
          pkgs.liburing
          self'.packages.rust-jemalloc-sys-unprefixed'
          rocksdbAllFeatures
        ];
        nativeBuildInputs = [
          pkgs.pkg-config
          # bindgen needs the build platform's libclang. Apparently due to "splicing
          # weirdness", pkgs.rustPlatform.bindgenHook on its own doesn't quite do the
          # right thing here.
          pkgs.rustPlatform.bindgenHook
        ];
        env = {
          LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
          LD_LIBRARY_PATH = lib.makeLibraryPath [
            pkgs.liburing
            self'.packages.rust-jemalloc-sys-unprefixed'
            rocksdbAllFeatures
          ];
        }
        // uwulib.environment.buildPackageEnv
        // {
          ROCKSDB_INCLUDE_DIR = "${rocksdbAllFeatures}/include";
          ROCKSDB_LIB_DIR = "${rocksdbAllFeatures}/lib";
        };
      };
      cargoArtifacts = self'.packages.continuwuity-all-features-deps;
    in
    {
      # taken from
      #
      # https://crane.dev/examples/quick-start.html
      checks = {
        continuwuity-all-features-build = self'.packages.continuwuity-all-features-bin;

        continuwuity-all-features-clippy = uwulib.build.craneLibForChecks.cargoClippy (
          commonAttrs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "-- --deny warnings";
          }
        );

        continuwuity-all-features-docs = uwulib.build.craneLibForChecks.cargoDoc (
          commonAttrs
          // {
            inherit cargoArtifacts;
            # This can be commented out or tweaked as necessary, e.g. set to
            # `--deny rustdoc::broken-intra-doc-links` to only enforce that lint
            env.RUSTDOCFLAGS = "--deny warnings";
          }
        );

        # Check formatting
        continuwuity-all-features-fmt = uwulib.build.craneLibForChecks.cargoFmt {
          src = uwulib.build.src;
        };

        continuwuity-all-features-toml-fmt = uwulib.build.craneLibForChecks.taploFmt {
          src = pkgs.lib.sources.sourceFilesBySuffices uwulib.build.src [ ".toml" ];
          # taplo arguments can be further customized below as needed
          # taploExtraArgs = "--config ./taplo.toml";
        };

        # Audit dependencies
        continuwuity-all-features-audit = uwulib.build.craneLibForChecks.cargoAudit {
          inherit (inputs) advisory-db;
          src = uwulib.build.src;
        };

        # Audit licenses
        continuwuity-all-features-deny = uwulib.build.craneLibForChecks.cargoDeny {
          src = uwulib.build.src;
        };

        # Run tests with cargo-nextest
        # Consider setting `doCheck = false` on `continuwuity-all-features` if you do not want
        # the tests to run twice
        continuwuity-all-features-nextest = uwulib.build.craneLibForChecks.cargoNextest (
          commonAttrs
          // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestPartitionsExtraArgs = "--no-tests=pass";
          }
        );
      };
    };
}
