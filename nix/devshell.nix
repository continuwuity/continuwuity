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
          pkgs.rust-jemalloc-sys-unprefixed
          pkgs.nodejs
          pkgs.liburing
          pkgs.pkg-config
        ];

        env = {
          LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
          LD_LIBRARY_PATH = lib.makeLibraryPath [
            pkgs.liburing
            pkgs.jemalloc
            pkgs.stdenv.cc.cc.lib
          ];
          PKG_CONFIG_PATH = lib.makeSearchPath "lib/pkgconfig" [
            pkgs.liburing.dev
          ];
        };
      };
    };
}
