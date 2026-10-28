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
            "x86_64-unknown-uefi"
            "x86_64-unknown-linux-gnu"
            "x86_64-unknown-none"
          ];
        }
      );

      rustPlatform = pkgs.makeRustPlatform {
        cargo = toolchain;
        rustc = toolchain;
      };

      bootloaderUefi =
        let
          src = pkgs.fetchCrate {
            pname = "bootloader-x86_64-uefi";
            version = "0.11.17";
            hash = "sha256-V+8JG5nAMPuj+CF2m01o3oqB9IKQPpKrAIYF2RtJPck=";
          };
        in
        rustPlatform.buildRustPackage {
          inherit src;
          pname = "bootloader-x86_64-uefi";
          version = "0.11.17";

          cargoLock.lockFile = "${src}/Cargo.lock";
          doCheck = false;
          dontFixup = true;
          auditable = false;

          buildPhase = ''
            runHook preBuild
            cargo build --release --locked --target x86_64-unknown-uefi
            runHook postBuild
          '';
          installPhase = ''
            install -D target/x86_64-unknown-uefi/release/bootloader-x86_64-uefi.efi \
              $out/bootloader-x86_64-uefi.efi
          '';
        };

      kernel = rustPlatform.buildRustPackage {
        pname = "benchix";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = [
          pkgs.makeWrapper
          pkgs.jq
        ];

        PREBUILT_UEFI = "${bootloaderUefi}/bootloader-x86_64-uefi.efi";
        RAMDISK_PATH = userspace.ramdisk;
        KERNEL_ARTIFACT_DIR = "${placeholder "out"}/share";

        # Very bad hacks
        preBuild = ''
          d=$NIX_BUILD_TOP/cargo-vendor-dir/bootloader-0.11.17
          sed -i \
            -e 's|Command::new(cargo)|Command::new("sh")|' \
            -e 's|cmd.arg("install").arg("bootloader-x86_64-uefi");|cmd.arg("-c").arg("mkdir -p $OUT_DIR/bin \&\& cp $PREBUILT_UEFI $OUT_DIR/bin/bootloader-x86_64-uefi.efi").arg("sh");|' \
            $d/build.rs
          jq -c '{files:{}, package:.package}' $d/.cargo-checksum.json > $d/c.json
          mv $d/c.json $d/.cargo-checksum.json
        '';

        # Sensible
        postInstall = ''
          wrapProgram $out/bin/benchix \
            --set UEFI_PATH $out/share/kernel.img \
            --set ELF_PATH $out/share/kernel.elf \
            --set OVMF_FD ${pkgs.OVMF.fd}/FV/OVMF.fd \
            --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.qemu ]}
        '';
      };
    in
    {
      packages.${system} = userspace // {
        inherit bootloaderUefi kernel;
        default = kernel;
      };

      apps.${system}.default = {
        type = "app";
        program = "${kernel}/bin/benchix";
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
