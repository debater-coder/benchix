{ pkgsStatic }:
let
  pkgFromName =
    name:
    pkgsStatic.stdenv.mkDerivation {
      pname = name;
      version = "0.1.0";
      src = ./src;

      buildPhase = ''
        $CC ${name}.c -o ${name}
      '';

      installPhase = ''
        mkdir -p $out/bin
        cp ${name} $out/bin
      '';
    };

  init = pkgFromName "init";
  echo = pkgFromName "echo";
  sh = pkgFromName "sh";
  userspace = pkgsStatic.symlinkJoin {
    name = "userspace";
    paths = [
      init
      echo
      sh
    ];
  };
in
{
  ramdisk = pkgsStatic.stdenv.mkDerivation {
    name = "ramdisk";

    buildCommand = ''
      mkdir -p $out/share
      tar -hcf $out/share/ramdisk.tar -C ${userspace}/bin .
    '';
  };
}
