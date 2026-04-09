build/ramdisk.tar: build/init
	tar -cf ramdisk.tar -C build/ .

build/init: src/init.c $(INCLUDE)
	nix-shell -p musl --command "musl-gcc -Wall -static src/init.c -o build/init"

clean:
	rm build/*
	rm ramdisk.tar
