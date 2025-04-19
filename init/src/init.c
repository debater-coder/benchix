#include <sys/syscall.h>

#define O_RDONLY	00000000
#define O_WRONLY	00000001
#define O_RDWR		00000002

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
         "pop %%rbp;" // C compiler will push rbp
         // "pop %%rdi;"        // argc
         // "mov %%rsp, %%rsi;" // argv
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

int main() {
    int fd = open("/dev/console", O_WRONLY);

    write(fd, "hello world\n", 12);
    return 0;
}
