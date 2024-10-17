{ rustPlatform, fetchFromGitHub, openssl, pkg-config }:

rustPlatform.buildRustPackage rec {
  pname = "backend";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "purduehackers";
    repo = "api";
    rev = "b48da90a0623d4af59261629396ccbe325ce8952";
    hash = "sha256-xvPpXpILJNrigl8MZQrakCcMJE8KI+BiPUBf+bOgQzM=";
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  cargoHash = "sha256-N+Ol92OhrRla/+G20+XtH9T1zUaKo06LoJIfzR5mPHA=";
  cargoDepsName = pname;
}
