init: src/init.c
	nix-shell -p glibc.static --run "cc -Os -s -static -nostartfiles src/init.c -o init"
