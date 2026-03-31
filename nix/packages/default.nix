{ inputs, ... }:
{
  perSystem =
    {
      craneLib,
      self',
      pkgs,
      lib,
      ...
    }:
    {
      packages =
        let
          src =
            let
              # see https://crane.dev/API.html#cranelibfiltercargosources
              # we need to keep the `web` directory which would be filtered out by the regular source filtering function
              # https://crane.dev/API.html#cranelibcleancargosource
              isWebTemplate = path: _type: builtins.match ".*(src/(web|service)|docs).*" path != null;
              isRust = craneLib.filterCargoSources;
              isNix = path: _type: builtins.match ".+/nix.*" path != null;
              webOrRustNotNix = p: t: !(isNix p t) && (isWebTemplate p t || isRust p t);
            in
            lib.cleanSourceWith {
              src = inputs.self;
              filter = webOrRustNotNix;
              name = "source";
            };

          common = {
            inherit src;
            nativeBuildInputs = with pkgs; [
              pkg-config
              rustPlatform.bindgenHook
            ];
            buildInputs = [ pkgs.liburing ];
            env.LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
          };

          cargoArtifacts = craneLib.buildDepsOnly common;

          rocksdb = pkgs.callPackage ./rocksdb.nix { };

          continuwuity = craneLib.buildPackage (
            lib.recursiveUpdate common {
              inherit cargoArtifacts;
              env = {
                ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
                ROCKSDB_LIB_DIR = "${rocksdb}/lib";
              };
            }
          );
        in
        {
          default = continuwuity;
          inherit rocksdb;
        };
    };
}
