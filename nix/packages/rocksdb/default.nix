{
  perSystem =
    {
      self',
      pkgs,
      ...
    }:
    {
      packages = {
        rocksdb = pkgs.callPackage ./package.nix {
          rust-jemalloc-sys-unprefixed = self'.packages.rust-jemalloc-sys-unprefixed';
        };
      };
    };
}
