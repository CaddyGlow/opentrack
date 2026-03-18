{
  description = "opentrack development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        llvm = pkgs.llvmPackages_21;
        libclang = llvm.libclang;
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.rustPlatform.bindgenHook
          ];

          packages = [
            pkgs.cargo
            pkgs.clippy
            pkgs.rustc
            pkgs.cmake
            pkgs.perl
            pkgs.pkg-config
            llvm.clang
            libclang
          ];

          LIBCLANG_PATH = "${libclang.lib}/lib";

          shellHook = ''
            export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$PWD/target}"
            echo "opentrack nix dev shell ready"
          '';
        };
      }
    );
}
