{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
    nativeBuildInputs = [
        pkgs.pkg-config
        pkgs.rustup
        pkgs.openssl
    ];
}
