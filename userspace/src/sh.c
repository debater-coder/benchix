#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

void help() {
  printf(
      "benchix sh (running in userspace). Type a command then press enter.\n");
}

char *concat(char *a, char *b) {
  uint64_t lena = strlen(a);
  uint64_t lenb = strlen(b);
  char *result = malloc(lena + lenb);

  memcpy(result, a, lena);
  memcpy(result + lena, b, lenb);

  return result;
}

// Destroys the original string
char **split(char *str, char delim) {
  uint64_t size = 0;
  char **result = NULL;

  while (*str != 0) {
    while (*str == delim && *str != 0) {
      *str = 0;
      str += 1;
    }
    if (*str == 0)
      break;

    // New non-delim token
    result = realloc(result, sizeof(char *) * (++size));
    if (result == NULL) {
      return NULL;
    }
    result[size - 1] = str;

    while (*str != delim && *str != 0) {
      str += 1;
    }
  }
  // null the end
  result = realloc(result, sizeof(char *) * (++size));
  if (result == NULL) {
    return NULL;
  }
  result[size - 1] = 0;

  return result;
}

bool interpret_cmd(char *line) {
  char **args = split(line, ' ');

  if (!args) {
    perror("sh");
    return false;
  }

  if (strcmp(args[0], "exit") == 0) {
    return false;
  } else if (strcmp(args[0], "help") == 0) {
    help();
  } else {
    if (args && args[0]) {
      int pid = fork();
      if (pid == 0) {
        execve(args[0], args, 0);
        char *path = concat("/bin/", args[0]);
        execve(path, args, 0);

        perror("sh");
        exit(-1);
      } else {
        waitpid(pid, NULL, 0);
      }

      if (pid < 0) {
        perror("sh");
      }
    }
  }
  free(args);

  return true;
}

int main(int argc, char *argv[]) {
  help();
  char cwd[100] = "/";

  bool running = true;
  char *line = NULL;
  size_t size = 0;
  size_t nread = 0;

  while (running) {
    printf("[benchix:%s]$ ", cwd);
    fflush(stdout);

    if ((nread = getline(&line, &size, stdin)) == -1) {
      if (feof(stdin)) {
        // handle end of file
        running = false;
      } else {
        perror("main");
        return -1;
      }
    }

    if (line[nread - 1] == '\n') {
      line[nread - 1] = 0;
    }

    if (running) {
      running = interpret_cmd(line);
    }
  }

  return 0;
}
