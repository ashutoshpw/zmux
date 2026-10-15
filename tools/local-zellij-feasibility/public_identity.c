/* Original MIT fixture. Public SDK APIs only; no production runtime linkage. */
#define _GNU_SOURCE 1
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#if defined(__APPLE__)
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>
#include <sys/time.h>
#include <sys/un.h>
#elif !defined(__linux__)
#error "This isolated fixture supports Darwin and Linux only"
#endif

/* This is our fixture ABI, not a redeclaration of an OS/kernel structure. */
struct zjfx_process {
    int64_t pid;
    uint64_t uid;
    uint64_t start_a;
    uint64_t start_b;
    char boot[80];
    char executable[4096];
};
struct zjfx_peer {
    int64_t pid;
    uint64_t uid;
    uint64_t gid;
};

size_t zjfx_process_size(void) { return sizeof(struct zjfx_process); }
size_t zjfx_peer_size(void) { return sizeof(struct zjfx_peer); }
const char *zjfx_api(void) {
#if defined(__APPLE__)
    return "getpeereid+SOL_LOCAL/LOCAL_PEERPID+proc_pidinfo/PROC_PIDTBSDINFO+proc_pidpath+kern.boottime";
#else
    return "SO_PEERCRED+/proc/pid/stat+/proc/pid/exe+/proc/sys/kernel/random/boot_id";
#endif
}

int zjfx_process_read(int64_t number, struct zjfx_process *out) {
    if (out == NULL || number <= 0 || number > INT_MAX) return EINVAL;
    memset(out, 0, sizeof(*out));
    pid_t pid = (pid_t)number;
    out->pid = number;
#if defined(__APPLE__)
    struct proc_bsdinfo info;
    memset(&info, 0, sizeof(info));
    int count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, (int)sizeof(info));
    if (count != (int)sizeof(info)) return errno != 0 ? errno : ESRCH;
    if ((int64_t)info.pbi_pid != number) return ESTALE;
    out->uid = info.pbi_uid;
    out->start_a = info.pbi_start_tvsec;
    out->start_b = info.pbi_start_tvusec;
    if (proc_pidpath(pid, out->executable, (uint32_t)sizeof(out->executable)) <= 0)
        return errno != 0 ? errno : ESRCH;
    struct timeval boot;
    size_t length = sizeof(boot);
    if (sysctlbyname("kern.boottime", &boot, &length, NULL, 0) != 0) return errno;
    if (length != sizeof(boot)) return EPROTO;
    int written = snprintf(out->boot, sizeof(out->boot), "%lld.%06d",
                           (long long)boot.tv_sec, (int)boot.tv_usec);
    if (written < 0 || (size_t)written >= sizeof(out->boot)) return EOVERFLOW;
#else
    char path[80], text[8192];
    int written = snprintf(path, sizeof(path), "/proc/%d/stat", (int)pid);
    if (written < 0 || (size_t)written >= sizeof(path)) return EOVERFLOW;
    FILE *file = fopen(path, "r");
    if (file == NULL) return errno;
    if (fgets(text, sizeof(text), file) == NULL) { fclose(file); return ESRCH; }
    fclose(file);
    char *end = strrchr(text, ')');
    if (end == NULL || end[1] != ' ') return EPROTO;
    char *state = NULL;
    char *token = strtok_r(end + 2, " \n", &state);
    for (int field = 3; field < 22 && token != NULL; ++field)
        token = strtok_r(NULL, " \n", &state);
    if (token == NULL) return EPROTO;
    errno = 0;
    char *after = NULL;
    unsigned long long start = strtoull(token, &after, 10);
    if (errno != 0 || after == token || *after != '\0') return EPROTO;
    out->start_a = start;
    (void)snprintf(path, sizeof(path), "/proc/%d", (int)pid);
    struct stat metadata;
    if (stat(path, &metadata) != 0) return errno;
    out->uid = metadata.st_uid;
    (void)snprintf(path, sizeof(path), "/proc/%d/exe", (int)pid);
    ssize_t size = readlink(path, out->executable, sizeof(out->executable) - 1);
    if (size < 0) return errno;
    if ((size_t)size >= sizeof(out->executable) - 1) return ENAMETOOLONG;
    out->executable[size] = '\0';
    file = fopen("/proc/sys/kernel/random/boot_id", "r");
    if (file == NULL) return errno;
    if (fgets(out->boot, sizeof(out->boot), file) == NULL) { fclose(file); return EIO; }
    fclose(file);
    out->boot[strcspn(out->boot, "\r\n")] = '\0';
#endif
    if (out->start_a == 0 || out->boot[0] == '\0' || out->executable[0] == '\0') return EPROTO;
    return 0;
}

int zjfx_peer_read(int fd, struct zjfx_peer *out) {
    if (out == NULL) return EINVAL;
    memset(out, 0, sizeof(*out));
    pid_t pid;
    uid_t uid;
    gid_t gid;
#if defined(__APPLE__)
    if (getpeereid(fd, &uid, &gid) != 0) return errno;
    socklen_t length = sizeof(pid);
    if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &pid, &length) != 0) return errno;
    if (length != sizeof(pid)) return EPROTO;
#else
    struct ucred credentials;
    socklen_t length = sizeof(credentials);
    if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &credentials, &length) != 0) return errno;
    if (length != sizeof(credentials)) return EPROTO;
    pid = credentials.pid;
    uid = credentials.uid;
    gid = credentials.gid;
#endif
    if (pid <= 0) return ENOTSUP;
    out->pid = pid;
    out->uid = uid;
    out->gid = gid;
    return 0;
}
