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

struct free_node {
  int size; // Inclusive of free_node contents
  struct free_node *next;
};

struct alloc_header {
  int size;
  int magic;
};

struct free_node *free_start = 0;

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

uint64_t execve(const char *pathname, char *const argv[], char *const envp[]) {
  asm __volatile__("movq $59, %rax;"
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

void *brk(void *addr) {
  asm __volatile__("movq $12, %rax;"
                   "syscall;"
                   "ret;");
}

void *sbrk(intptr_t increment) {
  void *curr_brk = brk(0);

  brk(curr_brk + increment);

  return curr_brk;
}

void *malloc(uint64_t size) {
  struct free_node *curr = free_start;
  struct free_node *prev = 0;

  while (curr != 0) {
    if (curr->size >= size + sizeof(struct alloc_header)) {
      if (prev != 0) {
        prev->next = curr->next; // Remove from freelist
      }
      ((struct alloc_header *)curr)->magic = 0xdeadbeef;
      ((struct alloc_header *)curr)->size = size;

      return (void *)(curr) + sizeof(struct alloc_header);
    }

    prev = curr;
    curr = curr->next;
  }

  struct alloc_header *alloc = sbrk(size + sizeof(struct alloc_header));

  alloc->size = size;
  alloc->magic = 0xdeadbeef;

  return (void *)(alloc) + sizeof(struct alloc_header);
}

void free(void *ptr) {
  if (ptr == 0) {
    return;
  }
  struct alloc_header *header = ptr - sizeof(struct alloc_header);

  if (header->magic != 0xdeadbeef) {
    puts("WARNING: non-malloc header passed to free()");
    return;
  }

  // Push free slot to start of free list
  ((struct free_node *)header)->size =
      header->size + sizeof(struct alloc_header);
  ((struct free_node *)header)->next = free_start;
  free_start = (struct free_node *)header;
}

void *memcpy(void *dest, void *src, uint64_t n) {
  for (int i = 0; i < n; i++) {
    ((char *)dest)[i] = ((char *)src)[i];
  }
}

void *realloc(void *ptr, uint64_t size) {
  void *alloc = malloc(size);

  if (ptr) {
    struct alloc_header *header = ptr - sizeof(struct alloc_header);
    if (header->magic != 0xdeadbeef) {
      puts("WARNING: non-malloc header passed to realloc()");
    } else {
      memcpy(alloc, ptr, header->size);
      free(ptr);
    }
  }

  return alloc;
}

char *getline(int fd) {
  char *line = 0;
  uint64_t size = 0;

  uint64_t len;
  do {
    line = realloc(line, size + 10);
    len = read(fd, line + size - 10, 10);

    size += len;

  } while (size > 0 && line[size - 1] != '\n' && line[size - 1] != 0);

  if (size > 0 && line[size - 1] == '\n') {
    line[size - 1] = 0;
  }

  return line;
}
