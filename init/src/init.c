#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

int main() {
    open("/dev/console", O_RDONLY); // fd 0 -- stdin
    open("/dev/console", O_WRONLY); // fd 1 -- stdout
    open("/dev/console", O_WRONLY); // fd 2 -- stderr

    write(1, "Hello, World!\n", 14);
    return 42;
}
