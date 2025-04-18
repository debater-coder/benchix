#include <stdio.h>
#include <fcntl.h>
#include <assert.h>
#include <unistd.h>

void main() {
    for (;;) {
        syscall(1, 2, 3, 4, 5);
    }
}
