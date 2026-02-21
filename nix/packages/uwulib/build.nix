args@{ pkgs, inputs, ... }:
let
  inherit (pkgs) lib;
  uwuenv = import ./environment.nix args;
  selfpkgs = inputs.self.packages.${pkgs.stdenv.system};
in
rec {
  # basic, very minimal instance of the crane library with a minimal rust toolchain
  craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (_: selfpkgs.build-toolchain);
  # the checks require more rust toolchain components, hence we have this separate instance of the crane library
  craneLibForChecks = (inputs.crane.mkLib pkgs).overrideToolchain (_: selfpkgs.dev-toolchain);

  # meta information (name, version, etc) of the rust crate based on the Cargo.toml
  crateInfo = craneLib.crateNameFromCargoToml { cargoToml = "${inputs.self}/Cargo.toml"; };

  src =
    let
      # see https://crane.dev/API.html#cranelibfiltercargosources
      #
      # we need to keep the `web` directory which would be filtered out by the regular source filtering function
      #
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

  # common attrs that are shared between building continuwuity's deps and the package itself
  commonAttrs =
    {
      profile ? "dev",
      ...
    }:
    {
      inherit (crateInfo)
        pname
        version
        ;
      inherit src;

      # this prevents unnecessary rebuilds
      strictDeps = true;

      dontStrip = profile == "dev" || profile == "test";
      dontPatchELF = profile == "dev" || profile == "test";

      doCheck = true;

      nativeBuildInputs = [
        # bindgen needs the build platform's libclang. Apparently due to "splicing
        # weirdness", pkgs.rustPlatform.bindgenHook on its own doesn't quite do the
        # right thing here.
        pkgs.rustPlatform.bindgenHook
      ];
    };

  makeRocksDBEnv =
    { rocksdb }:
    {
      ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
      ROCKSDB_LIB_DIR = "${rocksdb}/lib";
    };

  # function that builds the continuwuity dependencies derivation
  buildDeps =
    {
      rocksdb,
      features,
      commonAttrsArgs,
    }:
    craneLib.buildDepsOnly (
      (commonAttrs commonAttrsArgs)
      // {
        env = uwuenv.buildDepsOnlyEnv
              // (makeRocksDBEnv { inherit rocksdb; })
              // {
                # required since we started using unstable reqwest apparently ... otherwise the all-features build will fail
                RUSTFLAGS = "--cfg reqwest_unstable";
              };
        inherit (features) cargoExtraArgs;
      }

    );

  # function that builds the continuwuity package
  buildPackage =
    {
      deps,
      rocksdb,
      features,
      commonAttrsArgs,
    }:
    let
      rocksdbEnv = makeRocksDBEnv { inherit rocksdb; };
    in
    craneLib.buildPackage (
      (commonAttrs commonAttrsArgs)
      // {
        postFixup = ''
          patchelf --set-rpath "$(${pkgs.patchelf}/bin/patchelf --print-rpath $out/bin/${crateInfo.pname}):${rocksdb}/lib" $out/bin/${crateInfo.pname}
        '';
        cargoArtifacts = deps;
        doCheck = true;
        env =
          uwuenv.buildPackageEnv
          // rocksdbEnv
          // {
            # required since we started using unstable reqwest apparently ... otherwise the all-features build will fail
            RUSTFLAGS = "--cfg reqwest_unstable";
          };
        passthru.env = uwuenv.buildPackageEnv // rocksdbEnv;
        meta.mainProgram = crateInfo.pname;
        inherit (features) cargoExtraArgs;
      }
    );
}
