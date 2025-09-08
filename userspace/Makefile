INCLUDE := $(wildcard src/include/*.h)

build/ramdisk.tar: build/init build/sh build/echo
	tar -cf ramdisk.tar -C build/ .

build/init: src/init.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/init.c -o build/init

build/sh: src/sh.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/sh.c -o build/sh

build/echo: src/echo.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/echo.c -o build/echo

clean:
	rm build/*
	rm ramdisk.tar
