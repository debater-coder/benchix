#pragma once
#include <stdbool.h>
#include <stdint.h>

#define O_RDONLY 00000000
#define O_WRONLY 00000001
#define O_RDWR 00000002

#define STDIN_FD 0
#define STDOUT_FD 1
#define STDERR_FD 2

#define EOF -1

int main(int argc, char *argv[]);

void _start() {
  asm volatile("pop %rdi;"        // argc
               "movq %rsp, %rsi;" // argv
               "andq $-16, %rsp;" // align sp
               "call main;"
               "movq %rax, %rdi;"
               "movq $60, %rax;"
               "syscall;");
}

int open(char *path, int flags) {
  asm __volatile__("movq $2, %rax;"
                   "syscall;"
                   "ret;");
}

uint64_t read(int fd, void *buf, uint64_t count) {
  asm __volatile__("movq $0, %rax;"
                   "syscall;"
                   "ret;");
}

uint64_t write(int fd, void *buf, uint64_t count) {
  asm __volatile__("movq $1, %rax;"
                   "syscall;"
                   "ret;");
}

uint64_t strlen(char *str) {
  char *curr = str;
  while (*curr != '\0') {
    curr++;
  }
  return curr - str;
}

bool iserror(int64_t sysret_value) {
  return sysret_value <= -1 && sysret_value >= -4095;
}

int puts(char *str) {
  if (iserror(write(STDOUT_FD, str, strlen(str)))) {
    return EOF;
  }
  return 0;
}
