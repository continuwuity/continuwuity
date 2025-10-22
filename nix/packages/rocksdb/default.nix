{
  perSystem =
    {
      pkgs,
      ...
    }:
    {
      packages = {
        rocksdb = pkgs.callPackage ./package.nix { };
      };
    };
}
