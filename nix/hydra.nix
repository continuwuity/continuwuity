{ inputs, ... }:
let
  lib = inputs.nixpkgs.lib;
in
{
  flake.hydraJobs.packages = builtins.mapAttrs (
    _name: lib.hydraJob
  ) inputs.self.packages.x86_64-linux;
}
