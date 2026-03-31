{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      self',
      ...
    }:
    {
      _module.args.craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
        pkgs: self'.packages.stable-toolchain
      );
    };
}
