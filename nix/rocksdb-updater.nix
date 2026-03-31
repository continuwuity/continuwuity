{
  perSystem =
    { pkgs, ... }:
    {
      apps.update-rocksdb = {
        type = "app";
        program = pkgs.writeShellApplication {
          name = "update-rocksdb";
          runtimeInputs = [ pkgs.nix-update ];
          text = "nix-update rocksdb -F --version branch";
        };
      };
    };
}
