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
    in
    {
      # basic nix shell containing all things necessary to build continuwuity in all flavors manually (on x86_64-linux)
      devShells.default = uwulib.build.craneLib.devShell {
        packages = [
          pkgs.pkg-config
          pkgs.liburing
          pkgs.rust-jemalloc-sys-unprefixed
          rocksdbAllFeatures
        ];
        env.LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
      };
    };
}
