#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char *argv[]) {
  printf("Testing process cleanup...\n");

  char *args[] = {"/bin/hello", "--help"};

  for (int i = 0; i < 1000; i++) {
    printf("Iteration %d...\n", i);

    int pid = fork();

    if (pid == 0) {
      // Child
      execve("/bin/hello", args, 0);
      perror("test_process_cleanup");
      exit(-1);
    } else {
      // parent
      waitpid(pid, NULL, 0);
    }
  }

  printf("Success!\n");

  return 0;
}
