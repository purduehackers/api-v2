{ rustPlatform, fetchFromGitHub, openssl, pkg-config }:

rustPlatform.buildRustPackage rec {
  pname = "backend";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "purduehackers";
    repo = "api";
    rev = "c9d1815dc0f2242e7838f2788faff2aa42bd9e53";
    hash = "sha256-8szcy81DC6SrIkPXzey7tYXOshh9jJ08FYb1QDIRprM=";
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  cargoHash = "sha256-1ciCBFcJoelIWirrFgei5RArdmG/O5q5Zo/ZX/i0Dwc=";
  cargoDepsName = pname;
}
