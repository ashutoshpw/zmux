// SPDX-License-Identifier: MIT
// Real descriptor exhaustion at the transport API; run in a bounded subprocess.
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/resource.h>
#include <unistd.h>

typedef struct Pty Pty;
Pty *pty_new(const char *python, const char *script, const char *mode);
void pty_free(Pty *p);

int main(void) {
  struct rlimit limit;
  if (getrlimit(RLIMIT_NOFILE, &limit) || limit.rlim_max < 15) return 2;
  limit.rlim_cur = 15;
  if (setrlimit(RLIMIT_NOFILE, &limit)) return 2;
  // This executable has no inherited descriptors above stderr. Occupy 3..9 so
  // openpty + high-fd relocation + report pipe leave only one available slot.
  // The control pipe then fails atomically with EMFILE before any child exists.
  for (int fd = 3; fd < 15; fd++) {
    if (fcntl(fd, F_GETFD) != -1 || errno != EBADF) return 3;
  }
  for (int fd = 3; fd < 10; fd++) {
    if (open("/dev/null", O_RDONLY) != fd) return 4;
  }
  Pty *p = pty_new("/unused-python", "/unused-child", "unused-mode");
  if (p || errno != EMFILE) { pty_free(p); return 5; }
  for (int fd = 10; fd < 15; fd++) {
    if (fcntl(fd, F_GETFD) != -1 || errno != EBADF) return 6;
  }
  for (int fd = 3; fd < 10; fd++) {
    if (fcntl(fd, F_GETFD) == -1 || close(fd)) return 7;
  }
  puts("construction_exhaustion=true owned_fds_closed=true child_started=false");
  return 0;
}
