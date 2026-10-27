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

      rustPlatform = pkgs.makeRustPlatform {
        cargo = toolchain;
        rustc = toolchain;
      };

      kernel = rustPlatform.buildRustPackage {
        pname = "benchix";
        version = "0.1.0";
        src = ./.;

        cargoHash = "sha256-GC7UUYzS6vn0RdHfmrNAqaDR1D/vhYhRKMsNAf2bbJE=";
        nativeBuildInputs = [ pkgs.makeWrapper ];

        postInstall = ''
          mkdir -p $out/share
          cp $UEFI_PATH $out/share/kernel.img

          wrapProgram $out/bin/runner \
            --set UEFI_PATH $out/share/kernel.img
            --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.qemu ]}
        '';
      };

    in
    {
      packages.${system}.default = kernel;

      apps.${system}.default = {
        type = "app";
        program = "${kernel}/bin/runner";
      };

      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          qemu
          gdb
          toolchain
        ];
      };
    };
}
