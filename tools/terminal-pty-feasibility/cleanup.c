// SPDX-License-Identifier: MIT
// Observe actual owned-child state at the signal boundary, with no transport hook.
#define _DEFAULT_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static unsigned exited_signals, released_signals;
static int monitored_master = -1, signalled;
static unsigned waits_with_master_open;
static int observed_kill(pid_t pid, int value) {
  siginfo_t status;
  memset(&status, 0, sizeof(status));
  if (waitid(P_PID, (id_t)pid, &status, WEXITED | WNOHANG | WNOWAIT)) {
    // A released PID is no longer safe to signal, even in this regression.
    released_signals++;
    return -1;
  }
  if (status.si_pid == pid) exited_signals++;
  int result = kill(pid, value);
  if (result == 0) signalled = 1;
  return result;
}
static pid_t observed_waitpid(pid_t pid, int *status, int options) {
  if (signalled && monitored_master >= 0 && fcntl(monitored_master, F_GETFD) != -1)
    waits_with_master_open++;
  return waitpid(pid, status, options);
}
#define kill observed_kill
#define waitpid observed_waitpid
#undef _DEFAULT_SOURCE
#include "transport.c"
#undef kill
#undef waitpid

int main(int argc, char **argv) {
  int master_first = argc == 4 && strcmp(argv[2], "master-first") == 0;
  if (argc != 2 && !(argc == 3 && strcmp(argv[2], "reaped-error") == 0) && !master_first) return 2;
  Pty *p = pty_new(argv[1], master_first ? argv[3] : "/dev/null", master_first ? "stall" : "unused");
  if (!p) return 3;
  if (master_first) {
    // Of the parent's PTY + two-pipe descriptors, only the actual master is a
    // terminal. Observe its physical closure, without a production test export.
    for (int fd = 10; fd < 64; fd++) {
      if (isatty(fd)) {
        if (monitored_master >= 0) return 11;
        monitored_master = fd;
      }
    }
    if (monitored_master < 0) return 11;
    uint8_t ready = 0;
    unsigned turn;
    for (turn = 0; turn < 2000; turn++) {
      long n = pty_report(p, &ready, 1);
      if (n == 1) break;
      if (n != -2 && n != -3) return 12;
      (void)poll(NULL, 0, 1);
    }
    if (turn == 2000 || ready != 'R' || pty_write(p, (const uint8_t *)"PRE:", 4) != 4) return 12;
    for (turn = 0; turn < 2000; turn++) {
      int events = pty_poll(p, 1);
      if (events < 0) return 13;
      if (events & 1) break;
    }
    if (turn == 2000) return 13;
    int result = pty_stop(p);
    if (waits_with_master_open) {
      fputs("cleanup waited with owned PTY master still open\n", stderr);
      pty_free(p); return 10;
    }
    PtyStats stats;
    if (pty_stats(p, &stats, sizeof(stats)) || result || !signalled ||
        stats.child_live || stats.reaped != 1 || pty_closed_fds(p)) return 14;
    pty_free(p);
    puts("master_before_reap=true live_child_reaped=true owned_fds_closed=true");
    return 0;
  }
  siginfo_t status;
  unsigned turn;
  for (turn = 0; turn < 2000; turn++) {
    memset(&status, 0, sizeof(status));
    if (waitid(P_ALL, 0, &status, WEXITED | WNOHANG | WNOWAIT)) return 4;
    if (status.si_pid > 0) break;
    (void)poll(NULL, 0, 1);
  }
  if (turn == 2000 || status.si_code != CLD_EXITED || status.si_status != 0) return 5;
  if (argc == 3) {
    // Consume only this fixture's actual child to provoke genuine ECHILD at
    // the transport boundary. The owner must report it and never signal a
    // PID whose parent/child ownership proof has disappeared.
    if (waitid(P_PID, (id_t)status.si_pid, &status, WEXITED)) return 8;
  }
  int result = pty_stop(p);
  if (exited_signals || released_signals) {
    fputs("cleanup attempted a signal to an exited/released owned child\n", stderr);
    pty_free(p); return 6;
  }
  if (argc == 3) {
    PtyStats stats;
    if (pty_stats(p, &stats, sizeof(stats)) || result != -1 ||
        stats.cleanup_stage != CLEANUP_WAIT || stats.cleanup_errno != ECHILD ||
        stats.cleanup_errors != 1 || stats.open_fds != 0) return 9;
    pty_free(p);
    puts("wait_error_diagnostic=true operation=waitpid errno=ECHILD no_signal_attempt=true");
    return 0;
  }
  if (result || pty_closed_fds(p)) return 7;
  pty_free(p);
  puts("exited_child_cleanup=true no_signal_attempt=true owned_fds_closed=true");
  return 0;
}
