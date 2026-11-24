{
  rocksdb,
  fetchFromGitea,
  ...
}:
# RocksDB is built without jemalloc integration (the nixpkgs default). On Linux
# it still uses jemalloc at runtime, because continuwuity statically links an
# unprefixed jemalloc that replaces malloc/free process-wide. Linking RocksDB
# against a separate jemalloc puts two allocator instances in one process, which
# caused segfaults on db close/migration.
rocksdb.overrideAttrs ({
  version = "continuwuity-v0.5.0-unstable-2026-11-08";
  src = fetchFromGitea {
    domain = "forgejo.ellis.link";
    owner = "continuwuation";
    repo = "rocksdb";
    rev = "722fa3219df4169610fda8962221a1ff74026fdb";
    hash = "sha256-ZuXXp83Hgj0B5v499CNxa9oNQUS/ZKYOAZEhmarUifc=";
  };

  # We have this already at https://forgejo.ellis.link/continuwuation/rocksdb/commit/a935c0273e1ba44eacf88ce3685a9b9831486155
  # Unsetting `patches` so we don't have to revert it and make this nix exclusive
  patches = [ ];

  # Unset postPatch, as our version override breaks version-specific sed calls in the original package
  postPatch = "";
})
