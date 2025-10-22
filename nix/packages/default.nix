{
  imports = [
    ./continuwuity
    ./rocksdb
    ./rust.nix
    ./uwulib
  ];

  perSystem =
    { self', ... }:
    {
      packages.default = self'.packages.continuwuity-default-bin;
    };
}
