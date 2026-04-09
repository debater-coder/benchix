#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

int main(int argc, char *argv[]) {
  open("/dev/console", O_RDONLY); // fd 0 -- stdin
  open("/dev/console", O_WRONLY); // fd 1 -- stdout
  open("/dev/console", O_WRONLY); // fd 2 -- stderr

  puts("hello from init compiled with musl libc\n");

  return -1;
}
