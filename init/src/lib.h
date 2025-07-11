#pragma once
#include <stdint.h>

#define O_RDONLY 00000000
#define O_WRONLY 00000001
#define O_RDWR 00000002

#define SYS_exit 60

int main(int argc, char *argv[]);

void _start() {
  asm volatile("pop %%rdi;"         // argc
               "movq %%rsp, %%rsi;" // argv
               "andq $-16, %%rsp;"  // align sp
               "call main;"
               "movq %%rax, %%rdi;"
               "movq $60, %%rax;"
               "syscall;");
}

int open(char *path, int flags) {
  asm __volatile__("movq $2, %%rax;"
                   "syscall;"
                   "ret;");
}

uint64_t read(int fd, void *buf, uint64_t count) {
  asm __volatile__("movq $2, %%rax;"
                   "syscall;"
                   "ret;");
}

uint64_t write(int fd, void *buf, uint64_t count) {
  asm __volatile__("movq $2, %%rax;"
                   "syscall;"
                   "ret;");
}

uint64_t strlen(char *str) {
  char *curr = str;
  while (*curr) {
    str++;
  }
  return curr - str;
}
