{
  imports = [
    ./continuwuity
    ./rust.nix
    ./uwulib
  ];

  perSystem =
    { self', pkgs, ... }:
    {
      packages = {
        default = self'.packages.continuwuity-default-bin;
        rocksdb = pkgs.callPackage ./rocksdb.nix { };
      };
    };
}
