#include "include/lib.h"

int main(int argc, char *argv[]) {
  char cwd[100] = "/";
  char line[1024];

  for (;;) {
    puts("[benchix:");
    puts(cwd);
    puts("]$ ");
    read(0, line, 1024); // Read a line of text
  }

  return -1;
}
