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
  test_process_cleanup = pkgFromName "test_process_cleanup";
  mkUserspace =
    { pkgs }:
    pkgsStatic.symlinkJoin {
      name = "userspace";
      paths = pkgs;
    };
in
{
  ramdisk = pkgsStatic.lib.makeOverridable (
    (
      { pkgs }:
      pkgsStatic.stdenv.mkDerivation {
        name = "ramdisk";

        buildCommand = ''
          tar -hcf $out -C ${mkUserspace { inherit pkgs; }}/bin .
        '';
      }
    )
      {
        pkgs = [
          init
          echo
          sh
          test_process_cleanup
          pkgsStatic.hello
          pkgsStatic.busybox
        ];
      }
  );
}
