#include "include/lib.h"
#include <stdint.h>

void interpret_cmd(char *line) {
  char *path = concat("/bin/", line);

  char *args[] = {path, 0};
  execve(path, args, 0);

  puts("command not found\n");
}

int main(int argc, char *argv[]) {
  char cwd[100] = "/";

  for (;;) {
    puts("[benchix:");
    puts(cwd);
    puts("]$ ");
    char *line = getline(0);
    interpret_cmd(line);
    free(line);
  }

  return -1;
}
