{
  description = "Benchix OS Kernel";

  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.zst";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      system = "x86_64-linux";
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      userspace = import ./userspace/userspace.nix {
        pkgsStatic = nixpkgs.legacyPackages.${system}.pkgsStatic;
      };

      toolchain = pkgs.rust-bin.selectLatestNightlyWith (
        toolchain:
        toolchain.default.override {
          extensions = [ "rust-src" ];
          targets = [
            "x86_64-unknown-linux-gnu"
            "x86_64-unknown-none"
          ];
        }
      );
    in
    {
      packages.${system} = userspace;

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          qemu
          gdb
          toolchain
        ];
      };
    };
}
