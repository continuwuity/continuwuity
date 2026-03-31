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
          pkgs.nodejs
          pkgs.pkg-config
          pkgs.liburing
          pkgs.rust-jemalloc-sys-unprefixed
          self'.packages.rocksdb
        ];
        env.LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
      };
    };
}
