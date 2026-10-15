// SPDX-License-Identifier: MIT
// Original POSIX PTY fixture owner. No terminal parsing or application integration.
#define _DEFAULT_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#ifdef __APPLE__
#include <util.h>
#else
#include <pty.h>
#endif

typedef struct {
  uint64_t read_bytes, written_bytes, write_calls, eagain, short_writes;
  uint64_t open_fds, child_live, cleanup_errors, reaped, exit_code;
} PtyStats;
typedef struct {
  int fd[6], original[6]; // master/slave, report read/write, control read/write
  pid_t child;
  PtyStats stats;
  uint8_t saved_report[128];
  size_t saved_length;
  int report_nonblocking;
} Pty;
static int close_slot(Pty *p, size_t i) {
  if (p->fd[i] < 0) return 0;
  int fd = p->fd[i]; p->fd[i] = -1;
  p->stats.open_fds--;
  if (close(fd)) { p->stats.cleanup_errors++; return -1; }
  return 0;
}
static int high_fd(int raw) {
  int fd = fcntl(raw, F_DUPFD_CLOEXEC, 10);
  int saved = errno;
  if (close(raw) && fd >= 0) { close(fd); return -1; }
  errno = saved; return fd;
}
static int remember(Pty *p, size_t i, int raw) {
  int fd = high_fd(raw);
  if (fd < 0) return -1;
  p->fd[i] = p->original[i] = fd; p->stats.open_fds++; return 0;
}
static int nonblock(int fd) {
  int flags = fcntl(fd, F_GETFL);
  return flags < 0 ? -1 : fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}
static long io_result(ssize_t n) {
  if (n >= 0) return (long)n;
  if (errno == EAGAIN || errno == EWOULDBLOCK) return -2;
  if (errno == EINTR) return -3;
  if (errno == EIO) return -4;
  return -(100L + errno);
}
int pty_wait(Pty *p, unsigned milliseconds) {
  if (!p || milliseconds > 500) return -1;
  if (!p->child) return 1;
  for (unsigned i = 0; i <= milliseconds; i++) {
    int status = 0;
    pid_t result = waitpid(p->child, &status, WNOHANG);
    if (result == p->child) {
      p->child = 0; p->stats.child_live = 0; p->stats.reaped++;
      p->stats.exit_code = WIFEXITED(status) ? (uint64_t)WEXITSTATUS(status) :
        WIFSIGNALED(status) ? (uint64_t)(128 + WTERMSIG(status)) : 255;
      return 1;
    }
    if (result < 0 && errno != EINTR) { p->stats.cleanup_errors++; return -1; }
    if (i < milliseconds) (void)poll(NULL, 0, 1);
  }
  return 0;
}
int pty_close_master(Pty *p) { return p ? close_slot(p, 0) : -1; }
int pty_stop(Pty *p) {
  if (!p) return -1;
  if (p->child) {
    if (kill(p->child, SIGTERM) && errno != ESRCH) p->stats.cleanup_errors++;
    if (pty_wait(p, 50) == 0) {
      if (kill(p->child, SIGKILL) && errno != ESRCH) p->stats.cleanup_errors++;
      if (pty_wait(p, 200) != 1) p->stats.cleanup_errors++;
    }
  }
  if (p->report_nonblocking && p->fd[2] >= 0 && p->saved_length < sizeof(p->saved_report)) {
    ssize_t n = read(p->fd[2], p->saved_report + p->saved_length, sizeof(p->saved_report) - p->saved_length);
    if (n > 0) p->saved_length += (size_t)n;
    else if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) p->stats.cleanup_errors++;
  }
  for (size_t i = 0; i < 6; i++) close_slot(p, i);
  return p->child || p->stats.open_fds || p->stats.cleanup_errors ? -1 : 0;
}
void pty_free(Pty *p) { if (p) { pty_stop(p); free(p); } }
Pty *pty_new(const char *python, const char *script, const char *mode) {
  if (!python || !script || !mode) return NULL;
  Pty *p = calloc(1, sizeof(*p));
  if (!p) return NULL;
  for (size_t i = 0; i < 6; i++) p->fd[i] = p->original[i] = -1;
  int master = -1, slave = -1;
  struct termios raw;
  struct winsize size = {.ws_row = 3, .ws_col = 8};
  if (openpty(&master, &slave, NULL, NULL, &size)) goto fail;
  if (tcgetattr(slave, &raw)) { close(master); close(slave); goto fail; }
  cfmakeraw(&raw);
  if (tcsetattr(slave, TCSANOW, &raw)) { close(master); close(slave); goto fail; }
  if (remember(p, 0, master)) { close(slave); goto fail; }
  if (remember(p, 1, slave)) goto fail;
  for (size_t i = 2; i < 6; i += 2) {
    int pair[2];
    if (pipe(pair)) goto fail;
    if (remember(p, i, pair[0])) { close(pair[1]); goto fail; }
    if (remember(p, i + 1, pair[1])) goto fail;
  }
  if (nonblock(p->fd[0]) || nonblock(p->fd[2])) goto fail;
  p->report_nonblocking = 1;
  if (nonblock(p->fd[5])) goto fail;
  p->child = fork();
  if (p->child < 0) { p->child = 0; goto fail; }
  if (!p->child) {
    if (dup2(p->fd[1], 0) < 0 || dup2(p->fd[1], 1) < 0 || dup2(p->fd[1], 2) < 0 ||
        dup2(p->fd[3], 3) < 0 || dup2(p->fd[4], 4) < 0) _exit(126);
    for (size_t i = 0; i < 6; i++) close(p->fd[i]);
    // Inherit the fixture process group: timeout containment reaches this child.
    execl(python, python, "-B", script, mode, (char *)NULL);
    _exit(127);
  }
  p->stats.child_live = 1;
  close_slot(p, 1); close_slot(p, 3); close_slot(p, 4);
  if (p->stats.cleanup_errors) goto fail;
  return p;
fail: {
  int saved = errno;
  pty_free(p); errno = saved; return NULL;
}
}
long pty_read(Pty *p, uint8_t *out, size_t cap) {
  if (!p || !out || !cap || cap > 512 || p->fd[0] < 0) return -1;
  long n = io_result(read(p->fd[0], out, cap));
  if (n > 0) p->stats.read_bytes += (uint64_t)n;
  return n;
}
long pty_write(Pty *p, const uint8_t *bytes, size_t length) {
  if (!p || !bytes || !length || length > 8192 || p->fd[0] < 0) return -1;
  p->stats.write_calls++;
  long n = io_result(write(p->fd[0], bytes, length));
  if (n > 0) { p->stats.written_bytes += (uint64_t)n; if ((size_t)n < length) p->stats.short_writes++; }
  if (n == -2) p->stats.eagain++;
  return n;
}
long pty_report(Pty *p, uint8_t *out, size_t cap) {
  if (!p || !out || !cap || cap > 128) return -1;
  if (p->saved_length) {
    size_t n = p->saved_length < cap ? p->saved_length : cap;
    memcpy(out, p->saved_report, n);
    memmove(p->saved_report, p->saved_report + n, p->saved_length - n);
    p->saved_length -= n; return (long)n;
  }
  if (p->fd[2] < 0) return 0;
  return io_result(read(p->fd[2], out, cap));
}
int pty_control(Pty *p, uint8_t byte) {
  if (!p) return -1;
  return write(p->fd[5], &byte, 1) == 1 ? 0 : -1;
}
int pty_poll(Pty *p, unsigned milliseconds) {
  if (!p || milliseconds > 20 || p->fd[0] < 0) return -1;
  struct pollfd f = {.fd = p->fd[0], .events = POLLIN | POLLOUT};
  int n = poll(&f, 1, (int)milliseconds);
  if (n < 0) return errno == EINTR ? 0 : -1;
  return ((f.revents & POLLIN) ? 1 : 0) | ((f.revents & POLLOUT) ? 2 : 0) |
    ((f.revents & POLLHUP) ? 4 : 0) | ((f.revents & (POLLERR | POLLNVAL)) ? 8 : 0);
}
int pty_resize(Pty *p, uint16_t rows, uint16_t cols) {
  if (!p || !rows || !cols) return -1;
  struct winsize size = {.ws_row = rows, .ws_col = cols};
  return ioctl(p->fd[0], TIOCSWINSZ, &size) ? -1 : 0;
}
int pty_size(Pty *p, uint16_t *rows, uint16_t *cols) {
  if (!p || !rows || !cols) return -1;
  struct winsize size;
  if (ioctl(p->fd[0], TIOCGWINSZ, &size)) return -1;
  *rows = size.ws_row; *cols = size.ws_col; return 0;
}
int pty_stats(Pty *p, PtyStats *out, size_t size) {
  if (!p || !out || size != sizeof(*out)) return -1;
  *out = p->stats; return 0;
}
int pty_closed_fds(Pty *p) {
  if (!p || p->stats.open_fds || p->child) return -1;
  for (size_t i = 0; i < 6; i++) {
    if (p->original[i] >= 0 && (fcntl(p->original[i], F_GETFD) != -1 || errno != EBADF)) return -1;
  }
  return 0;
}
size_t pty_fixed_bytes(void) { return sizeof(Pty); }
