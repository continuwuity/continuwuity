{ inputs, ... }:
{
  flake.uwulib = {
    init = pkgs: {
      features = import ./features.nix { inherit pkgs inputs; };
      environment = import ./environment.nix { inherit pkgs inputs; };
      build = import ./build.nix { inherit pkgs inputs; };
    };
  };
}
