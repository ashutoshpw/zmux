/* Independent finite owner used by the packaged profile acceptance journey. */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#else
#define _POSIX_C_SOURCE 200809L
#endif
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>

int main(int count, char **arguments) {
    if (count != 3 || signal(SIGALRM, SIG_DFL) == SIG_ERR) { return 2; }
    alarm(60);
    int lifetime = open(arguments[1], O_RDWR | O_NOFOLLOW);
    if (lifetime < 0 || flock(lifetime, LOCK_SH | LOCK_NB) != 0) { return 3; }
    struct stat identity;
    if (fstat(lifetime, &identity) != 0 || !S_ISREG(identity.st_mode)
        || identity.st_uid != getuid() || identity.st_size != 0) { return 4; }
    int ready = open(arguments[2], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0600);
    if (ready < 0 || write(ready, "ready\n", 6) != 6 || fsync(ready) != 0
        || close(ready) != 0) { return 5; }
    char byte;
    ssize_t result;
    do { result = read(STDIN_FILENO, &byte, 1); } while (result < 0 && errno == EINTR);
    if (result != 0 || close(lifetime) != 0) { return 6; }
    return 0;
}
