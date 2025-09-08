#include "include/lib.h"

int main(int argc, char *argv[]) {
  open("/dev/console", O_RDONLY); // fd 0 -- stdin
  open("/dev/console", O_WRONLY); // fd 1 -- stdout
  open("/dev/console", O_WRONLY); // fd 2 -- stderr

  char *args[] = {"/bin/sh", 0};
  execve("/bin/sh", args, 0);

  return -1;
}
