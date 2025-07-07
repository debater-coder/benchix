#include <sys/syscall.h>

// Theres no libc so lets take some things from musl

#define O_RDONLY	00000000
#define O_WRONLY	00000001
#define O_RDWR		00000002

/**
 * Gets length of c-str not including null terminator.
 */
unsigned long long int my_strlen(char* str) {
    char* curr_ptr = str;

    while (*curr_ptr != '\0') {
        curr_ptr += 1;
    }

    return curr_ptr - str;
}

// https://news.ycombinator.com/item?id=8975209
#define sysdef(name)               \
    int name() {                   \
        __asm__ __volatile__(      \
            "movq %0, %%rax;"      \
            "mov %%rcx, %%r10;"    \
            "syscall;"             \
            :: "i"(SYS_##name) : "rax" \
        );                         \
    }

void _start() {
    __asm__ __volatile__ (
         "pop %%rdi;"        // argc
         "mov %%rsp, %%rsi;" // argv = thing at sp
         "andq $-16, %%rsp;"
         "call main;"
         "movq %%rax, %%rdi;" // exit
         "movq %0, %%rax;"
         "syscall;"
         :: "i"(SYS_exit)
    );
}

sysdef(write);
sysdef(open);


int main(int argc, char** argv) {
    int fd = open("/dev/console", O_WRONLY);

    write(fd, "hello world\n", 12);
    for (int i = 0; i < argc; i++) {
        write(fd, argv[i], my_strlen(argv[i]));
        write(fd, "\n", 1);
    }
    return 32;
}
