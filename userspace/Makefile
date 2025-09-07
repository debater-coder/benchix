INCLUDE := $(wildcard src/include/*.h)

all: build/init build/ls

build/init: src/init.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/init.c -o build/init

build/ls: src/ls.c $(INCLUDE)
	gcc -Wall -nostdlib -fno-stack-protector src/ls.c -o build/ls
