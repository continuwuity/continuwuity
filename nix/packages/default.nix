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

          rocksdb = pkgs.callPackage ./rocksdb.nix { };

          attrs = {
            inherit src;
            nativeBuildInputs = with pkgs; [
              pkg-config
              rustPlatform.bindgenHook
            ];
            buildInputs = [
              pkgs.liburing
            ];
            env = {
              ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
              ROCKSDB_LIB_DIR = "${rocksdb}/lib";
            };
          };

          cargoArtifacts = craneLib.buildDepsOnly attrs;
        in
        {
          default = craneLib.buildPackage (
            lib.recursiveUpdate attrs {
              inherit cargoArtifacts;

              # Needed to make continuwuity link to rocksdb
              postFixup = ''
                old_rpath="$(patchelf --print-rpath $out/bin/conduwuit)"
                extra_rpath="${
                  pkgs.lib.makeLibraryPath [
                    pkgs.rocksdb
                  ]
                }"

                patchelf  --set-rpath "$old_rpath:$extra_rpath" $out/bin/conduwuit
              '';

              meta = {
                description = "A community-driven Matrix homeserver in Rust";
                mainProgram = "conduwuit";
                platforms = lib.platforms.linux;
                maintainers = with lib.maintainers; [ quadradical ];
              };
            }
          );
          inherit rocksdb;
        };
    };
}
