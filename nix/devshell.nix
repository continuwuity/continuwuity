{
  perSystem =
    {
      craneLib,
      self',
      lib,
      pkgs,
      ...
    }:
    {
      # basic nix shell containing all things necessary to build continuwuity in all flavors manually (on x86_64-linux)
      devShells.default = craneLib.devShell {
        packages = [
          self'.packages.rocksdb
          pkgs.nodejs
          pkgs.pkg-config
        ]
        ++ lib.optionals pkgs.stdenv.isLinux [
          pkgs.liburing
          pkgs.rust-jemalloc-sys-unprefixed
        ];

        env = {
          LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
          LD_LIBRARY_PATH = lib.makeLibraryPath (
            [
              pkgs.stdenv.cc.cc.lib
            ]
            ++ lib.optionals pkgs.stdenv.isLinux [
              pkgs.liburing
              pkgs.jemalloc
            ]
          );
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          PKG_CONFIG_PATH = lib.makeSearchPath "lib/pkgconfig" [
            pkgs.liburing.dev
          ];
        };
      };
    };
}
