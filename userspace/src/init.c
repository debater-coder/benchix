#include "include/lib.h"
#include "stddef.h"

int main(int argc, char *argv[]) {
  open("/dev/console", O_RDONLY); // fd 0 -- stdin
  open("/dev/console", O_WRONLY); // fd 1 -- stdout
  open("/dev/console", O_WRONLY); // fd 2 -- stderr

  puts("execve to /init/init\n");

  char *args[] = {"/init/init", "test1", "test2"};
  execve("/init/init", args, NULL);

  return 42;
}
