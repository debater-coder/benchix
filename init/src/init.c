#include "lib.h"

int main(int argc, char *argv[]) {
  open("/dev/console", O_RDONLY); // fd 0 -- stdin
  open("/dev/console", O_WRONLY); // fd 1 -- stdout
  open("/dev/console", O_WRONLY); // fd 2 -- stderr

  puts("Hello\n");

  for (int i = 0; i < argc; i++) {
    puts(argv[i]);
    puts("\n");
  }

  return 42;
}
