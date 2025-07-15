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

  char *buf[100];

  for (;;) {
    puts(">");
    int count = read(STDIN_FD, buf, 100);

    if (count == 0) {
      break;
    }

    write(STDOUT_FD, buf, count);
  }

  return 42;
}
