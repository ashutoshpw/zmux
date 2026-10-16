/* Independent, finite process probing a real mounted WebKit profile's lease. */
#if defined(__APPLE__)
/* flock and O_NOFOLLOW are Darwin extensions under strict C11 compilation. */
#define _DARWIN_C_SOURCE
#else
#define _POSIX_C_SOURCE 200809L
#endif
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/file.h>
#include <unistd.h>
int main(int count, char **arguments) {
    if (signal(SIGALRM, SIG_DFL) == SIG_ERR) { return 5; }
    alarm(5);
    if (count != 2) { return 2; }
    int descriptor = open(arguments[1], O_RDWR | O_NOFOLLOW);
    if (descriptor < 0) { return 3; }
    int result = flock(descriptor, LOCK_EX | LOCK_NB);
    int busy = result < 0 && (errno == EWOULDBLOCK || errno == EAGAIN);
    close(descriptor);
    return busy ? 0 : 4;
}
