{ rustPlatform, fetchFromGitHub, openssl, pkg-config }:

rustPlatform.buildRustPackage rec {
  pname = "purdue-hackers-api";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "purduehackers";
    repo = "api";
    rev = "88a813f92f6ea3d65ef0b454ac19b5564af185fb";
    hash = "";
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  cargoHash = "";
  cargoDepsName = pname;
}
