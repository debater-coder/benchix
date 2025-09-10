#include "include/lib.h"
#include <stdint.h>

void interpret_cmd(char *line) {
  char path[1020] = "/bin/";
  memcpy(path + 5, line, strlen(line));

  char *args[] = {path, 0};
  execve(path, args, 0);

  puts("command not found\n");
}

int main(int argc, char *argv[]) {
  char cwd[100] = "/";
  char line[1000];

  for (;;) {
    puts("[benchix:");
    puts(cwd);
    puts("]$ ");
    uint64_t len = read(0, line, 999);
    line[len - 1] = 0;

    interpret_cmd(line);
  }

  return -1;
}
