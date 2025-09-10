#include "include/lib.h"

int main(int argc, char *argv[]) {
  if (argc >= 2) {
    puts(argv[1]);
  }

  for (int i = 2; i < argc; i++) {
    puts(" ");
    puts(argv[i]);
  }

  puts("\n");
  return 45;
}
