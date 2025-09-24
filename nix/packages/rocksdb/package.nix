{
  lib,
  stdenv,

  rocksdb,
  liburing,
  rust-jemalloc-sys-unprefixed,

  enableJemalloc ? false,
  enableLiburing ? false,

  fetchFromGitea,

  ...
}:
let
  notDarwin = !stdenv.hostPlatform.isDarwin;
in
(rocksdb.override {
  # Override the liburing input for the build with our own so
  # we have it built with the library flag
  inherit liburing;
  jemalloc = rust-jemalloc-sys-unprefixed;

  # rocksdb fails to build with prefixed jemalloc, which is required on
  # darwin due to [1]. In this case, fall back to building rocksdb with
  # libc malloc. This should not cause conflicts, because all of the
  # jemalloc symbols are prefixed.
  #
  # [1]: https://github.com/tikv/jemallocator/blob/ab0676d77e81268cd09b059260c75b38dbef2d51/jemalloc-sys/src/env.rs#L17
  enableJemalloc = enableJemalloc && notDarwin;

  # for some reason enableLiburing in nixpkgs rocksdb is default true
  # which breaks Darwin entirely
  enableLiburing = enableLiburing && notDarwin;
}).overrideAttrs
  (old: {
    src = fetchFromGitea {
      domain = "forgejo.ellis.link";
      owner = "continuwuation";
      repo = "rocksdb";
      rev = "10.5.fb";
      sha256 = "sha256-X4ApGLkHF9ceBtBg77dimEpu720I79ffLoyPa8JMHaU=";
    };
    version = "10.5.fb";
    cmakeFlags =
      lib.subtractLists (builtins.map (flag: lib.cmakeBool flag true) [
        # No real reason to have snappy or zlib, no one uses this
        "WITH_SNAPPY"
        "ZLIB"
        "WITH_ZLIB"
        # We don't need to use ldb or sst_dump (core_tools)
        "WITH_CORE_TOOLS"
        # We don't need to build rocksdb tests
        "WITH_TESTS"
        # We use rust-rocksdb via C interface and don't need C++ RTTI
        "USE_RTTI"
        # This doesn't exist in RocksDB, and USE_SSE is deprecated for
        # PORTABLE=$(march)
        "FORCE_SSE42"
      ]) old.cmakeFlags
      ++ (builtins.map (flag: lib.cmakeBool flag false) [
        # No real reason to have snappy, no one uses this
        "WITH_SNAPPY"
        "ZLIB"
        "WITH_ZLIB"
        # We don't need to use ldb or sst_dump (core_tools)
        "WITH_CORE_TOOLS"
        # We don't need trace tools
        "WITH_TRACE_TOOLS"
        # We don't need to build rocksdb tests
        "WITH_TESTS"
        # We use rust-rocksdb via C interface and don't need C++ RTTI
        "USE_RTTI"
      ]);

    enableLiburing = enableLiburing && notDarwin;

    # outputs has "tools" which we don't need or use
    outputs = [ "out" ];

    # preInstall hooks has stuff for messing with ldb/sst_dump which we don't need or use
    preInstall = "";

    # We have this already at https://forgejo.ellis.link/continuwuation/rocksdb/commit/a935c0273e1ba44eacf88ce3685a9b9831486155
    # Unsetting `patches` so we don't have to revert it and make this nix exclusive
    patches = [ ];
  })
