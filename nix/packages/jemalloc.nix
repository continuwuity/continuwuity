{
  perSystem =
    {
      pkgs,
      ...
    }:
    {
      # we disable some unused features here. The code won't compile without these
      #
      # > <jemalloc>: Invalid conf pair: prof_active:false
      # > error: test failed, to rerun pass `-p conduwuit --lib`
      # >
      # > Caused by:
      # >   process didn't exit successfully: `/build/source/target/release/deps/conduwuit-67fbd204f38e8c35` (signal: 11, SIGSEGV: invalid memory reference)
      packages.rust-jemalloc-sys-unprefixed' = pkgs.rust-jemalloc-sys-unprefixed.overrideAttrs (old: {
        configureFlags =
          old.configureFlags
          ++
            # we dont need docs
            [ "--disable-doc" ]
          ++
            # we dont need cxx/C++ integration
            [ "--disable-cxx" ];
      });
    };
}
