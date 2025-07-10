#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>

int main(int argc, char *argv[]) {
    open("/dev/console", O_RDONLY); // fd 0 -- stdin
    open("/dev/console", O_WRONLY); // fd 1 -- stdout
    open("/dev/console", O_WRONLY); // fd 2 -- stderr

    for (int i = 0; i < argc; i++) {
        write(1, argv[i], strlen(argv[i]));
    }

    return 42;
}
