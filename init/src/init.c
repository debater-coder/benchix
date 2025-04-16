#include <sys/syscall.h>
#include <unistd.h>

void _start() {
    for (int i = 0; i < 32; i++) {
        syscall(1, 0);
    }

    for (;;) {
        syscall(1, 2, 3, 4, 5);
    }
}
