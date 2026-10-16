/* Real foreign process: native leases outlive logical Desktop release. */
#ifdef __APPLE__
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L
#define _XOPEN_SOURCE 700
#include "zmux.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static char *consume(ZmuxResult result) {
    assert(result.status == ZMUX_OK && result.value != 0);
    ZmuxResult length = zmux_buffer_length(result.value);
    assert(length.status == ZMUX_OK && length.value < 8388608);
    char *text = calloc((size_t)length.value + 1, 1);
    assert(text != NULL);
    assert(zmux_buffer_copy(result.value, (uint8_t *)text, length.value) == ZMUX_OK);
    assert(zmux_buffer_release(result.value) == ZMUX_OK);
    return text;
}
static char *snapshot(uint64_t desktop) {
    for (int attempt = 0; attempt < 500; attempt++) {
        ZmuxResult result = zmux_desktop_snapshot(desktop, 0);
        if (result.status == ZMUX_OK && result.value != 0) { return consume(result); }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(!"native Desktop snapshot did not publish within its bound");
    return NULL;
}
static char *lease(uint64_t desktop, const char *request) {
    return consume(zmux_desktop_profile_lease(desktop, (const uint8_t *)request, strlen(request)));
}
static char *native(const char *request) {
    return consume(zmux_desktop_profile_native((const uint8_t *)request, strlen(request)));
}
static void native_ok(const char *request) {
    char *response = native(request);
    assert(strstr(response, "\"ok\":true") != NULL);
    free(response);
}
static char *object_after(const char *text, const char *key) {
    const char *start = strstr(text, key);
    assert(start != NULL);
    start += strlen(key);
    assert(*start == '{');
    const char *end = start;
    int depth = 0;
    do { if (*end == '{') { depth++; } else if (*end == '}') { depth--; } end++; } while (depth > 0 && *end);
    assert(depth == 0);
    size_t length = (size_t)(end - start);
    char *copy = calloc(length + 1, 1);
    assert(copy != NULL);
    memcpy(copy, start, length);
    return copy;
}
static void assert_exclusive_busy(const char *path) {
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        /* No allocator/library work after fork from the threaded foreign host. */
        int descriptor = open(path, O_RDWR | O_NOFOLLOW);
        if (descriptor < 0) { _exit(2); }
        int locked = flock(descriptor, LOCK_EX | LOCK_NB);
        int busy = locked < 0 && (errno == EWOULDBLOCK || errno == EAGAIN);
        close(descriptor);
        _exit(busy ? 0 : 3);
    }
    int status = 0;
    for (int attempt = 0; attempt < 200; attempt++) {
        pid_t done = waitpid(child, &status, WNOHANG);
        assert(done >= 0);
        if (done == child) { assert(WIFEXITED(status) && WEXITSTATUS(status) == 0); return; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(kill(child, SIGKILL) == 0);
    assert(waitpid(child, &status, 0) == child);
    assert(!"independent lease contender exceeded its bound");
}
int main(int count, char **arguments) {
    assert(count == 2);
    /* Python owns allocation and cleanup of this exact canonical private root. */
    char *root = realpath(arguments[1], NULL);
    assert(root != NULL && !strcmp(root, arguments[1]));
    struct stat fixture;
    assert(stat(root, &fixture) == 0 && S_ISDIR(fixture.st_mode));
    assert(fixture.st_uid == geteuid() && (fixture.st_mode & 07777) == 0700);
    char catalog[4096], locks[4096], configuration[8192];
    assert(snprintf(catalog, sizeof catalog, "%s/catalog", root) > 0);
    assert(snprintf(locks, sizeof locks, "%s/leases", root) > 0);
    int size = snprintf(configuration, sizeof configuration,
        "{\"state_directory\":\"%s\",\"profile_lease_directory\":\"%s\"}", catalog, locks);
    assert(size > 0 && (size_t)size < sizeof configuration);
    ZmuxResult desktop = zmux_desktop_create_configured((const uint8_t *)configuration, (size_t)size);
    assert(desktop.status == ZMUX_OK);
    for (int attempt = 0; ; attempt++) {
        assert(attempt < 500);
        char *text = snapshot(desktop.value);
        int ready = strstr(text, "\"startup_pending\":false") != NULL &&
            strstr(text, "\"loading\":false") != NULL;
        free(text);
        if (ready) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    const uint8_t name[] = "Native lease profile";
    char *admission = consume(zmux_desktop_profile_add(desktop.value, name, sizeof name - 1));
    assert(strstr(admission, "\"ok\":true") != NULL);
    free(admission);
    char profile[37] = {0};
    for (int attempt = 0; ; attempt++) {
        assert(attempt < 500);
        char *text = snapshot(desktop.value);
        char *identity = strstr(text, "\"profiles\":[{\"id\":\"");
        if (identity) { memcpy(profile, identity + strlen("\"profiles\":[{\"id\":\""), 36); }
        free(text);
        if (profile[0]) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    const char *owner = "{\"epoch\":\"09fccf38-d31e-49aa-8d1d-20a740dc1ef1\",\"store\":\"8ec9813f-3a24-4ecf-99ac-977a86619d43\"}";
    char request[2048], status_request[512], lock_path[8192];
    snprintf(request, sizeof request, "{\"operation\":\"acquire\",\"owner\":%s,\"profile\":\"%s\"}", owner, profile);
    snprintf(status_request, sizeof status_request, "{\"operation\":\"status\",\"owner\":%s}", owner);
    assert(zmux_desktop_profile_lease(desktop.value, NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_profile_lease(desktop.value, (const uint8_t *)request, 2049).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_profile_lease(desktop.value, (const uint8_t *)"{}", 2).status == ZMUX_INVALID_ARGUMENT);
    uint64_t buffers[16];
    ZmuxResult core = zmux_core_create();
    assert(core.status == ZMUX_OK);
    const uint8_t query[] = "{\"version\":1,\"command\":{\"method\":\"snapshot\"}}";
    for (int index = 0; index < 16; index++) {
        ZmuxResult held = zmux_core_dispatch(core.value, query, sizeof query - 1);
        assert(held.status == ZMUX_OK);
        buffers[index] = held.value;
    }
    assert(zmux_desktop_profile_lease(desktop.value, (const uint8_t *)request, strlen(request)).status == ZMUX_CAPACITY);
    for (int index = 0; index < 16; index++) { assert(zmux_buffer_release(buffers[index]) == ZMUX_OK); }
    assert(zmux_core_destroy(core.value) == ZMUX_OK);
    char *status = lease(0, status_request);
    assert(strstr(status, "\"ok\":true") && strstr(status, "\"token\":null"));
    free(status);
    char *acquired = lease(desktop.value, request);
    assert(strstr(acquired, "\"ok\":true") && strstr(acquired, "\"mode\":\"shared\""));
    char *token = strdup(strstr(acquired, "\"token\":") + strlen("\"token\":"));
    assert(token != NULL && token[0] == '{');
    token[strlen(token) - 1] = '\0'; /* remove only the outer response object */
    free(acquired);
    snprintf(lock_path, sizeof lock_path, "%s/browser-profile-%s.lock", locks, profile);
    struct stat before;
    assert(stat(lock_path, &before) == 0 && before.st_size == 0);
    assert_exclusive_busy(lock_path);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
    assert_exclusive_busy(lock_path);
    status = lease(0, status_request);
    assert(strstr(status, token) != NULL);
    free(status);
    snprintf(request, sizeof request, "{\"operation\":\"release_unconstructed\",\"token\":%s}", token);
    char *released = lease(0, request);
    assert(strstr(released, "\"ok\":true"));
    free(released);
    status = lease(0, status_request);
    assert(strstr(status, "\"token\":null"));
    free(status);
    struct stat after;
    assert(stat(lock_path, &after) == 0 && before.st_dev == after.st_dev && before.st_ino == after.st_ino);

    /* Trusted ABI state-machine harness: these acknowledgments deliberately do
       not claim WebKit execution. The native macOS owner supplies real callbacks. */
    const char *controller = "de591758-a0c5-4f11-a0cd-4a2260c912c4";
    size = snprintf(configuration, sizeof configuration,
        "{\"state_directory\":\"%s\",\"profile_lease_directory\":\"%s\",\"profile_controller\":\"%s\"}", catalog, locks, controller);
    desktop = zmux_desktop_create_configured((const uint8_t *)configuration, (size_t)size);
    assert(desktop.status == ZMUX_OK);
    for (int attempt = 0; ; attempt++) {
        assert(attempt < 500);
        char *text = snapshot(desktop.value);
        int ready = strstr(text, "\"startup_pending\":false") && strstr(text, "\"loading\":false");
        free(text);
        if (ready) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    snprintf(request, sizeof request, "{\"operation\":\"delete\",\"profile\":\"%s\"}", profile);
    admission = consume(zmux_desktop_profile_command(desktop.value, (const uint8_t *)request, strlen(request)));
    assert(strstr(admission, "\"ok\":true") && strstr(admission, "\"state\":\"pending\""));
    char intent[37] = {0};
    const char *intent_start = strstr(admission, "\"intent\":\"");
    assert(intent_start != NULL);
    memcpy(intent, intent_start + strlen("\"intent\":\""), 36);
    free(admission);
    snprintf(request, sizeof request, "{\"operation\":\"drain\",\"controller\":\"%s\"}", controller);
    assert(zmux_desktop_profile_native(NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_profile_native((const uint8_t *)"{}", 2).status == ZMUX_INVALID_ARGUMENT);
    core = zmux_core_create();
    assert(core.status == ZMUX_OK);
    for (int index = 0; index < 16; index++) {
        ZmuxResult held = zmux_core_dispatch(core.value, query, sizeof query - 1);
        assert(held.status == ZMUX_OK); buffers[index] = held.value;
    }
    assert(zmux_desktop_profile_native((const uint8_t *)request, strlen(request)).status == ZMUX_CAPACITY);
    for (int index = 0; index < 16; index++) { assert(zmux_buffer_release(buffers[index]) == ZMUX_OK); }
    assert(zmux_core_destroy(core.value) == ZMUX_OK);
    char *batch = native(request);
    assert(strstr(batch, intent) && strstr(batch, "\"kind\":\"delete\""));
    free(batch);
    batch = native(request);
    assert(strstr(batch, "\"requests\":[]"));
    free(batch);
    snprintf(request, sizeof request, "{\"operation\":\"begin\",\"controller\":\"%s\",\"intent\":\"%s\",\"stores\":[]}", controller, intent);
    char *began = native(request);
    assert(strstr(began, "\"ok\":true"));
    char *writer = object_after(began, "\"token\":");
    free(began);
    assert(stat(lock_path, &after) == 0 && after.st_ino == before.st_ino && after.st_size > 0);
    snprintf(request, sizeof request, "{\"operation\":\"advance\",\"token\":%s,\"phase\":\"clear_dispatched\"}", writer);
    native_ok(request);
    native_ok(request); /* acknowledgment is idempotent, not another effect */
    snprintf(request, sizeof request, "{\"operation\":\"finish\",\"token\":%s,\"result\":{\"outcome\":\"aborted_before_clear\",\"message\":\"generic timeout after dispatch\"}}", writer);
    char *refused = native(request);
    assert(strstr(refused, "\"ok\":false"));
    free(refused);
    assert(stat(lock_path, &after) == 0 && after.st_size > 0);
    const char *phases[] = { "clear_completed", "remove_dispatched", "remove_completed" };
    for (int phase = 0; phase < 3; phase++) {
        snprintf(request, sizeof request, "{\"operation\":\"advance\",\"token\":%s,\"phase\":\"%s\"}", writer, phases[phase]);
        native_ok(request);
    }
    snprintf(request, sizeof request, "{\"operation\":\"finish\",\"token\":%s,\"result\":{\"outcome\":\"completed\"}}", writer);
    native_ok(request);
    assert(stat(lock_path, &after) == 0 && after.st_ino == before.st_ino && after.st_size == 0);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
    snprintf(request, sizeof request, "{\"operation\":\"status\",\"controller\":\"%s\",\"profile\":\"%s\",\"intent\":\"%s\"}", controller, profile, intent);
    status = native(request);
    assert(strstr(status, "\"phase\":\"completed\"") && strstr(status, "\"physical_cleanup_pending\":false"));
    free(status);
    free(writer);
    free(token);
    /* The process-scoped service exits before its runner removes the known root. */
    puts("native_profile_lease_passed");
    free(root);
    return 0;
}
