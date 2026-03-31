{
  stdenv,
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
  enableJemalloc = stdenv.hostPlatform.isLinux;
}).overrideAttrs
  ({
    version = "continuwuity-v0.5.0-unstable-2026-03-27";
    src = fetchFromGitea {
      domain = "forgejo.ellis.link";
      owner = "continuwuation";
      repo = "rocksdb";
      rev = "463f47afceebfe088f6922420265546bd237f249";
      hash = "sha256-1ef75IDMs5Hba4VWEyXPJb02JyShy5k4gJfzGDhopRk=";
    };

    # We have this already at https://forgejo.ellis.link/continuwuation/rocksdb/commit/a935c0273e1ba44eacf88ce3685a9b9831486155
    # Unsetting `patches` so we don't have to revert it and make this nix exclusive
    patches = [ ];
  })
