build/ramdisk.tar: build/init build/echo build/sh
	tar -cf ramdisk.tar -C build/ .

build/init: src/init.c $(INCLUDE)
	nix-shell -p musl --command "musl-gcc -Wall -static src/init.c -o build/init"

build/sh: src/sh.c $(INCLUDE)
	nix-shell -p musl --command "musl-gcc -Wall -static src/sh.c -o build/sh"

build/echo: src/echo.c $(INCLUDE)
	nix-shell -p musl --command "musl-gcc -Wall -static src/echo.c -o build/echo"
clean:
	rm build/*
	rm ramdisk.tar
