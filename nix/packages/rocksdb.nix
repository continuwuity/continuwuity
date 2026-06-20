{
  # stdenv,
  # enableJemalloc ? stdenv.hostPlatform.isLinux,
  enableJemalloc ? false,
  rocksdb,
  fetchFromGitea,
  rust-jemalloc-sys-unprefixed,
  ...
}:
(rocksdb.override {
  # rocksdb fails to build with prefixed jemalloc, which is required on
  # darwin due to [1]. In this case, fall back to building rocksdb with
  # libc malloc. This should not cause conflicts, because all of the
  # jemalloc symbols are prefixed.
  #
  # [1]: https://github.com/tikv/jemallocator/blob/ab0676d77e81268cd09b059260c75b38dbef2d51/jemalloc-sys/src/env.rs#L17
  jemalloc = rust-jemalloc-sys-unprefixed;
  inherit enableJemalloc;
}).overrideAttrs
  ({
    version = "continuwuity-v0.5.0-unstable-2026-05-19";
    src = fetchFromGitea {
      domain = "forgejo.ellis.link";
      owner = "continuwuation";
      repo = "rocksdb";
      rev = "3756b2b905e13216d8b56bcc783d814e7b073aff";
      hash = "sha256-rSv4fr2bf9JJwdodgeuPCuceeh7k97KVxrAOC0wyPQY=";
    };

    # We have this already at https://forgejo.ellis.link/continuwuation/rocksdb/commit/a935c0273e1ba44eacf88ce3685a9b9831486155
    # Unsetting `patches` so we don't have to revert it and make this nix exclusive
    patches = [ ];

    # Unset postPatch, as our version override breaks version-specific sed calls in the original package
    postPatch = "";
  })
