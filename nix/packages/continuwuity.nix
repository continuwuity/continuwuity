{
  lib,
  self,
  stdenv,
  liburing,
  craneLib,
  pkg-config,
  callPackage,
  rustPlatform,
  cargoExtraArgs ? "",
  rocksdb ? callPackage ./rocksdb.nix { },
}:
let
  # see https://crane.dev/API.html#cranelibfiltercargosources
  # we need to keep the `web` directory which would be filtered out by the regular source filtering function
  # https://crane.dev/API.html#cranelibcleancargosource
  isWebTemplate = path: _type: builtins.match ".*(src/(web|service)|docs).*" path != null;
  isRust = craneLib.filterCargoSources;
  isNix = path: _type: builtins.match ".+/nix.*" path != null;
  webOrRustNotNix = p: t: !(isNix p t) && (isWebTemplate p t || isRust p t);

  src = lib.cleanSourceWith {
    src = self;
    filter = webOrRustNotNix;
    name = "source";
  };

  attrs = {
    inherit src;
    nativeBuildInputs = [
      pkg-config
      rustPlatform.bindgenHook
    ];
    buildInputs = lib.optionals stdenv.hostPlatform.isLinux [ liburing ];
    env = {
      ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
      ROCKSDB_LIB_DIR = "${rocksdb}/lib";
    };
  };
in
craneLib.buildPackage (
  lib.recursiveUpdate attrs {
    inherit cargoExtraArgs;
    cargoArtifacts = craneLib.buildDepsOnly attrs;

    # Needed to make continuwuity link to rocksdb
    postFixup = ''
      old_rpath="$(patchelf --print-rpath $out/bin/conduwuit)"
      extra_rpath="${
        lib.makeLibraryPath [
          rocksdb
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
)
