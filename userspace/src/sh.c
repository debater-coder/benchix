#include "include/lib.h"

bool running = true; // yes its a global variable

void exit_sh() { running = false; }

void help() {
  puts("Benchix sh (running in userspace). Type a command then press enter.\n");
}

void interpret_cmd(char *line) {
  char **args = split(line, ' ');

  if (streq(args[0], "exit")) {
    exit_sh();
  } else if (streq(args[0], "help")) {
    help();
  } else {
    if (args && args[0]) {

      // This will work if args is a real path
      execve(args[0], args, 0);

      // ok then try /bin/...
      char *path = concat("/bin/", args[0]);
      args[0] = path;        // Update to true path of executable
      execve(path, args, 0); // No env for now

      // no
      puts("command not found\n");

      // this will go in other branch of fork()
      free(path);
    }
  }
  free(args);
}

int main(int argc, char *argv[]) {
  help();
  char cwd[100] = "/";

  while (running) {
    // Prompt
    puts("[benchix:");
    puts(cwd);
    puts("]$ ");

    char *line = getline(0);
    interpret_cmd(line);
    free(line);
  }

  return 0;
}
