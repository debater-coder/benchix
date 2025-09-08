INCLUDE := $(wildcard src/include/*.h)

build/ramdisk.tar: build/init build/ls
	tar -cf ramdisk.tar -C build/ .

build/init: src/init.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/init.c -o build/init

build/ls: src/ls.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/ls.c -o build/ls
