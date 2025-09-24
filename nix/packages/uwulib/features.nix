{ pkgs, inputs, ... }:
let
  inherit (pkgs) lib;
in
rec {
  defaultDisabledFeatures = [
    # dont include experimental features
    "experimental"
    # jemalloc profiling/stats features are expensive and shouldn't
    # be expected on non-debug builds.
    "jemalloc_prof"
    "jemalloc_stats"
    # this is non-functional on nix for some reason
    "hardened_malloc"
    # conduwuit_mods is a development-only hot reload feature
    "conduwuit_mods"
    # we don't want to enable this feature set by default but be more specific about it
    "full"
  ];
  # We perform default-feature unification in nix, because some of the dependencies
  # on the nix side depend on feature values.
  calcFeatures =
    {
      tomlPath ? "${inputs.self}/src/main",
      # either a list of feature names or a string "all" which enables all non-default features
      enabledFeatures ? [ ],
      disabledFeatures ? defaultDisabledFeatures,
      default_features ? true,
      disable_release_max_log_level ? false,
    }:
    let
      # simple helper to get the contents of a Cargo.toml file in a nix format
      getToml = path: lib.importTOML "${path}/Cargo.toml";

      # get all the features except for the default features
      allFeatures = lib.pipe tomlPath [
        getToml
        (manifest: manifest.features)
        lib.attrNames
        (lib.remove "default")
      ];

      # get just the default enabled features
      allDefaultFeatures = lib.pipe tomlPath [
        getToml
        (manifest: manifest.features.default)
      ];

      # depending on the value of enabledFeatures choose just a set or all non-default features
      #
      # - [ list of features ] -> choose exactly the features listed
      # - "all" -> choose all non-default features
      additionalFeatures = if enabledFeatures == "all" then allFeatures else enabledFeatures;

      # unification with default features (if enabled)
      features = lib.unique (additionalFeatures ++ lib.optionals default_features allDefaultFeatures);

      # prepare the features that are subtracted from the set
      disabledFeatures' =
        disabledFeatures ++ lib.optionals disable_release_max_log_level [ "release_max_log_level" ];

      # construct the final feature set
      finalFeatures = lib.subtractLists disabledFeatures' features;
    in
    {
      # final feature set, useful for querying it
      features = finalFeatures;

      # crane flag with the relevant features
      cargoExtraArgs = builtins.concatStringsSep " " [
        "--no-default-features"
        "--locked"
        (lib.optionalString (finalFeatures != [ ]) "--features")
        (builtins.concatStringsSep "," finalFeatures)
      ];
    };
}
