{
  lib,
  self,
  stdenv,
  rocksdb,
  craneLib,
  pkg-config,
  liburing,
  rustPlatform,
  cargoExtraArgs ? "",
  rustflags ? "",
  target_cpu ? null,
  profile ? "release",
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
    __structuredAttrs = true;
    strictDeps = true;

    inherit src;

    nativeBuildInputs = [
      pkg-config
      rustPlatform.bindgenHook
    ];

    buildInputs = lib.optionals stdenv.hostPlatform.isLinux [ liburing ];

    env = {
      CARGO_PROFILE = profile;
      # Rust links with its bundled rust-lld by default on linux-gnu targets.
      # This bypasses nixpkgs' linker wrapper (and its rpaths), so we make it use the system one.
      # `-C linker-features=-lld` is only stable on x86_64, but cc takes the last `-fuse-ld`.
      RUSTFLAGS = lib.concatStringsSep " " (
        lib.optional stdenv.hostPlatform.isLinux "-C link-arg=-fuse-ld=bfd" ++ [ rustflags ]
      );
      GIT_COMMIT_HASH = self.rev or self.dirtyRev or "";
      GIT_COMMIT_HASH_SHORT = self.shortRev or self.dirtyShortRev or "";
    }
    // (lib.optionalAttrs (rocksdb != null) {
      ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
      ROCKSDB_LIB_DIR = "${rocksdb}/lib";
    })
    // (lib.optionalAttrs (target_cpu != null) {
      TARGET_CPU = target_cpu;
    });
  };
in
craneLib.buildPackage (
  lib.recursiveUpdate attrs {
    inherit cargoExtraArgs;
    cargoArtifacts = craneLib.buildDepsOnly attrs;

    meta = {
      description = "A community-driven Matrix homeserver in Rust";
      mainProgram = "conduwuit";
      platforms = lib.platforms.all;
      maintainers = with lib.maintainers; [ quadradical ];
    };
  }
)
