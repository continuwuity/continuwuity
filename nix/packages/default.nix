{ inputs, ... }:
{
  perSystem =
    {
      self',
      pkgs,
      lib,
      ...
    }:
    {
      packages =
        let
          crane = self'.packages.crane;
          src =
            let
              # see https://crane.dev/API.html#cranelibfiltercargosources
              # we need to keep the `web` directory which would be filtered out by the regular source filtering function
              # https://crane.dev/API.html#cranelibcleancargosource
              isWebTemplate = path: _type: builtins.match ".*(src/(web|service)|docs).*" path != null;
              isRust = crane.filterCargoSources;
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
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.liburing ];
            env.LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
          };

          cargoArtifacts = crane.buildDepsOnly common;

          continuwuity = crane.buildPackage (
            common
            // {
              inherit cargoArtifacts;
              doCheck = false;
              env.LIBCLANG_PATH = lib.makeLibraryPath [ pkgs.llvmPackages.libclang.lib ];
            }
          );
        in
        {
          default = continuwuity;
          rocksdb = pkgs.callPackage ./rocksdb.nix { };
        };
    };
}
