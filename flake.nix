{
  inputs = {
    attic.url = "github:zhaofengli/attic?ref=main";
    cachix.url = "github:cachix/cachix?ref=master";
    crane = {
      url = "github:ipetkov/crane?ref=master";
    };
    fenix = {
      url = "github:nix-community/fenix?ref=main";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-compat = {
      url = "github:edolstra/flake-compat?ref=master";
      flake = false;
    };
    flake-utils.url = "github:numtide/flake-utils?ref=main";
    nix-filter.url = "github:numtide/nix-filter?ref=main";
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixpkgs-unstable";
    rocksdb = {
      url = "git+https://forgejo.ellis.link/continuwuation/rocksdb?ref=10.4.fb";
      flake = false;
    };
  };

  outputs =
    inputs:
    inputs.flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgsHost = import inputs.nixpkgs {
          inherit system;
        };

        # The Rust toolchain to use
        toolchain = inputs.fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;

          # See also `rust-toolchain.toml`
          sha256 = "sha256-KUm16pHj+cRedf8vxs/Hd2YWxpOrWZ7UOrwhILdSJBU=";
        };

        mkScope =
          pkgs:
          pkgs.lib.makeScope pkgs.newScope (self: {
            inherit pkgs inputs;
            craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (_: toolchain);
            main = self.callPackage ./nix/pkgs/main { };
            liburing = pkgs.liburing.overrideAttrs {
              # Tests weren't building
              outputs = [
                "out"
                "dev"
                "man"
              ];
              buildFlags = [ "library" ];
            };
            rocksdb =
              (pkgs.rocksdb_9_10.override {
                # Override the liburing input for the build with our own so
                # we have it built with the library flag
                inherit (self) liburing;
              }).overrideAttrs
                (old: {
                  src = inputs.rocksdb;
                  version = "v10.4.fb";
                  cmakeFlags =
                    pkgs.lib.subtractLists [
                      # No real reason to have snappy or zlib, no one uses this
                      "-DWITH_SNAPPY=1"
                      "-DZLIB=1"
                      "-DWITH_ZLIB=1"
                      # We don't need to use ldb or sst_dump (core_tools)
                      "-DWITH_CORE_TOOLS=1"
                      # We don't need to build rocksdb tests
                      "-DWITH_TESTS=1"
                      # We use rust-rocksdb via C interface and don't need C++ RTTI
                      "-DUSE_RTTI=1"
                      # This doesn't exist in RocksDB, and USE_SSE is deprecated for
                      # PORTABLE=$(march)
                      "-DFORCE_SSE42=1"
                      # PORTABLE will get set in main/default.nix
                      "-DPORTABLE=1"
                    ] old.cmakeFlags
                    ++ [
                      # No real reason to have snappy, no one uses this
                      "-DWITH_SNAPPY=0"
                      "-DZLIB=0"
                      "-DWITH_ZLIB=0"
                      # We don't need to use ldb or sst_dump (core_tools)
                      "-DWITH_CORE_TOOLS=0"
                      # We don't need trace tools
                      "-DWITH_TRACE_TOOLS=0"
                      # We don't need to build rocksdb tests
                      "-DWITH_TESTS=0"
                      # We use rust-rocksdb via C interface and don't need C++ RTTI
                      "-DUSE_RTTI=0"
                    ];

                  # outputs has "tools" which we don't need or use
                  outputs = [ "out" ];

                  # preInstall hooks has stuff for messing with ldb/sst_dump which we don't need or use
                  preInstall = "";

                  # We have this already at https://forgejo.ellis.link/continuwuation/rocksdb/commit/a935c0273e1ba44eacf88ce3685a9b9831486155
                  # Unsetting this so we don't have to revert it and make this nix exclusive
                  patches = [ ];

                  postPatch = ''
                    # Fix gcc-13 build failures due to missing <cstdint> and
                    # <system_error> includes, fixed upstream since 8.x
                    sed -e '1i #include <cstdint>' -i db/compaction/compaction_iteration_stats.h
                    sed -e '1i #include <cstdint>' -i table/block_based/data_block_hash_index.h
                    sed -e '1i #include <cstdint>' -i util/string_util.h
                    sed -e '1i #include <cstdint>' -i include/rocksdb/utilities/checkpoint.h
                  '';
                });
          });

        scopeHost = mkScope pkgsHost;
        mkCrossScope =
          crossSystem:
          let
            pkgsCrossStatic =
              (import inputs.nixpkgs {
                inherit system;
                crossSystem = {
                  config = crossSystem;
                };
              }).pkgsStatic;
          in
          mkScope pkgsCrossStatic;

      in
      {
        packages =
          {
            default = scopeHost.main.override {
              disable_features = [
                # Don't include experimental features
                "experimental"
                # jemalloc profiling/stats features are expensive and shouldn't
                # be expected on non-debug builds.
                "jemalloc_prof"
                "jemalloc_stats"
                # This is non-functional on nix for some reason
                "hardened_malloc"
                # conduwuit_mods is a development-only hot reload feature
                "conduwuit_mods"
              ];
            };
            default-debug = scopeHost.main.override {
              profile = "dev";
              # Debug build users expect full logs
              disable_release_max_log_level = true;
              disable_features = [
                # Don't include experimental features
                "experimental"
                # This is non-functional on nix for some reason
                "hardened_malloc"
                # conduwuit_mods is a development-only hot reload feature
                "conduwuit_mods"
              ];
            };
            # Just a test profile used for things like CI and complement
            default-test = scopeHost.main.override {
              profile = "test";
              disable_release_max_log_level = true;
              disable_features = [
                # Don't include experimental features
                "experimental"
                # this is non-functional on nix for some reason
                "hardened_malloc"
                # conduwuit_mods is a development-only hot reload feature
                "conduwuit_mods"
              ];
            };
            all-features = scopeHost.main.override {
              all_features = true;
              disable_features = [
                # Don't include experimental features
                "experimental"
                # jemalloc profiling/stats features are expensive and shouldn't
                # be expected on non-debug builds.
                "jemalloc_prof"
                "jemalloc_stats"
                # This is non-functional on nix for some reason
                "hardened_malloc"
                # conduwuit_mods is a development-only hot reload feature
                "conduwuit_mods"
              ];
            };
            all-features-debug = scopeHost.main.override {
              profile = "dev";
              all_features = true;
              # Debug build users expect full logs
              disable_release_max_log_level = true;
              disable_features = [
                # Don't include experimental features
                "experimental"
                # This is non-functional on nix for some reason
                "hardened_malloc"
                # conduwuit_mods is a development-only hot reload feature
                "conduwuit_mods"
              ];
            };
            hmalloc = scopeHost.main.override { features = [ "hardened_malloc" ]; };
          }
          // builtins.listToAttrs (
            builtins.concatLists (
              builtins.map
                (
                  crossSystem:
                  let
                    binaryName = "static-${crossSystem}";
                    scopeCrossStatic = mkCrossScope crossSystem;
                  in
                  [
                    # An output for a statically-linked binary
                    {
                      name = binaryName;
                      value = scopeCrossStatic.main;
                    }

                    # An output for a statically-linked binary with x86_64 haswell
                    # target optimisations
                    {
                      name = "${binaryName}-x86_64-haswell-optimised";
                      value = scopeCrossStatic.main.override {
                        x86_64_haswell_target_optimised =
                          if (crossSystem == "x86_64-linux-gnu" || crossSystem == "x86_64-linux-musl") then true else false;
                      };
                    }

                    # An output for a statically-linked unstripped debug ("dev") binary
                    {
                      name = "${binaryName}-debug";
                      value = scopeCrossStatic.main.override {
                        profile = "dev";
                        # debug build users expect full logs
                        disable_release_max_log_level = true;
                      };
                    }

                    # An output for a statically-linked unstripped debug binary with the
                    # "test" profile (for CI usage only)
                    {
                      name = "${binaryName}-test";
                      value = scopeCrossStatic.main.override {
                        profile = "test";
                        disable_release_max_log_level = true;
                        disable_features = [
                          # dont include experimental features
                          "experimental"
                          # this is non-functional on nix for some reason
                          "hardened_malloc"
                          # conduwuit_mods is a development-only hot reload feature
                          "conduwuit_mods"
                        ];
                      };
                    }

                    # An output for a statically-linked binary with `--all-features`
                    {
                      name = "${binaryName}-all-features";
                      value = scopeCrossStatic.main.override {
                        all_features = true;
                        disable_features = [
                          # dont include experimental features
                          "experimental"
                          # jemalloc profiling/stats features are expensive and shouldn't
                          # be expected on non-debug builds.
                          "jemalloc_prof"
                          "jemalloc_stats"
                          # this is non-functional on nix for some reason
                          "hardened_malloc"
                          # conduwuit_mods is a development-only hot reload feature
                          "conduwuit_mods"
                        ];
                      };
                    }

                    # An output for a statically-linked binary with `--all-features` and with x86_64 haswell
                    # target optimisations
                    {
                      name = "${binaryName}-all-features-x86_64-haswell-optimised";
                      value = scopeCrossStatic.main.override {
                        all_features = true;
                        disable_features = [
                          # dont include experimental features
                          "experimental"
                          # jemalloc profiling/stats features are expensive and shouldn't
                          # be expected on non-debug builds.
                          "jemalloc_prof"
                          "jemalloc_stats"
                          # this is non-functional on nix for some reason
                          "hardened_malloc"
                          # conduwuit_mods is a development-only hot reload feature
                          "conduwuit_mods"
                        ];
                        x86_64_haswell_target_optimised =
                          if (crossSystem == "x86_64-linux-gnu" || crossSystem == "x86_64-linux-musl") then true else false;
                      };
                    }

                    # An output for a statically-linked unstripped debug ("dev") binary with `--all-features`
                    {
                      name = "${binaryName}-all-features-debug";
                      value = scopeCrossStatic.main.override {
                        profile = "dev";
                        all_features = true;
                        # debug build users expect full logs
                        disable_release_max_log_level = true;
                        disable_features = [
                          # dont include experimental features
                          "experimental"
                          # this is non-functional on nix for some reason
                          "hardened_malloc"
                          # conduwuit_mods is a development-only hot reload feature
                          "conduwuit_mods"
                        ];
                      };
                    }

                    # An output for a statically-linked binary with hardened_malloc
                    {
                      name = "${binaryName}-hmalloc";
                      value = scopeCrossStatic.main.override {
                        features = [ "hardened_malloc" ];
                      };
                    }
                  ]
                )
                [
                  #"x86_64-apple-darwin"
                  #"aarch64-apple-darwin"
                  "x86_64-linux-gnu"
                  "x86_64-linux-musl"
                  "aarch64-linux-musl"
                ]
            )
          );
      }
    );
}
