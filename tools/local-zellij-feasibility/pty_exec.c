/* Original MIT fixture: acquire a real controlling terminal before exec. */
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 2 || !isatty(STDIN_FILENO)) {
        fprintf(stderr, "PTY fixture requires an executable and terminal stdin\n");
        return 111;
    }
    if (ioctl(STDIN_FILENO, TIOCSCTTY, 0) != 0 ||
        tcsetpgrp(STDIN_FILENO, getpgrp()) != 0) {
        fprintf(stderr, "acquire fixture controlling terminal: %s\n", strerror(errno));
        return 111;
    }
    execv(argv[1], argv + 1);
    fprintf(stderr, "exec fixture: %s\n", strerror(errno));
    return 111;
}
