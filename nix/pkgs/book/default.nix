{ inputs

  # Dependencies
, main
, mdbook
, stdenv
}:

stdenv.mkDerivation {
  inherit (main) pname version;

  src = inputs.nix-filter {
    root = inputs.self;
    include = [
      "book.toml"
      "conduwuit-example.toml"
      "CODE_OF_CONDUCT.md"
      "CONTRIBUTING.md"
      "README.md"
      "development.md"
      "debian/conduwuit.service"
      "debian/README.md"
      "arch/conduwuit.service"
      "docs"
      "theme"
    ];
  };

  nativeBuildInputs = [
    mdbook
  ];

  buildPhase = ''
    mdbook build -d $out
  '';
}
