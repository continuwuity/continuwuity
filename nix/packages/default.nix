{
  self,
  ...
}:
{
  perSystem =
    {
      pkgs,
      craneLib,
      ...
    }:
    {

      packages = {
        rocksdb = pkgs.callPackage ./rocksdb.nix { };
        default = pkgs.callPackage ./continuwuity.nix { inherit self craneLib; };
      };
    };
}
