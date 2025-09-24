args@{ pkgs, inputs, ... }:
let
  uwubuild = import ./build.nix args;
in
rec {
  buildDepsOnlyEnv = {
    # https://crane.dev/faq/rebuilds-bindgen.html
    NIX_OUTPATH_USED_AS_RANDOM_SEED = "aaaaaaaaaa";
    CARGO_PROFILE = "release";
  }
  // uwubuild.craneLib.mkCrossToolchainEnv (p: pkgs.clangStdenv);

  buildPackageEnv = {
    GIT_COMMIT_HASH = inputs.self.rev or inputs.self.dirtyRev or "";
    GIT_COMMIT_HASH_SHORT = inputs.self.shortRev or inputs.self.dirtyShortRev or "";
  }
  // buildDepsOnlyEnv;
}
