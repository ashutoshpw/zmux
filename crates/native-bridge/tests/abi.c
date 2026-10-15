/* Independent C caller: exercises the actual shared-library boundary and owned buffers. */
#ifdef __APPLE__
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L
#define _XOPEN_SOURCE 700
#include "zmux.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <time.h>
#include <unistd.h>
#include <dirent.h>
#include <sys/stat.h>

/* Foreign hosts do not inherit Rust main's ignored SIGPIPE disposition. Closing an
 * already-exited terminal must survive that default and leave the host alive. */
static char *desktop_snapshot(uint64_t desktop) {
    for (int attempt = 0; attempt < 500; attempt++) {
        ZmuxResult result = zmux_desktop_snapshot(desktop, 0);
        if (result.status == ZMUX_OK && result.value != 0) {
            ZmuxResult length = zmux_buffer_length(result.value);
            assert(length.status == ZMUX_OK);
            char *text = calloc((size_t)length.value + 1, 1);
            assert(text != NULL);
            assert(zmux_buffer_copy(result.value, (uint8_t *)text, (size_t)length.value) == ZMUX_OK);
            assert(zmux_buffer_release(result.value) == ZMUX_OK);
            return text;
        }
        assert(result.status == ZMUX_BUSY || result.status == ZMUX_OK);
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(!"Desktop snapshot timed out");
    return NULL;
}
static void exited_desktop_survives_close(void) {
    assert(signal(SIGPIPE, SIG_DFL) != SIG_ERR);
    ZmuxResult desktop = zmux_desktop_create();
    assert(desktop.status == ZMUX_OK);
    uint64_t session = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        char *text = desktop_snapshot(desktop.value);
        char *id = strstr(text, "\"session_id\":");
        if (id != NULL && strstr(text, "\"state\":\"running\"") != NULL) {
            session = strtoull(id + strlen("\"session_id\":"), NULL, 10);
        }
        free(text);
        if (session != 0) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(session != 0);
    char request[256];
    snprintf(request, sizeof request, "{\"version\":1,\"request_id\":1,\"payload\":{\"command\":\"input\",\"session_id\":%llu,\"text\":\"exit\\r\"}}", (unsigned long long)session);
    ZmuxResult result = zmux_desktop_dispatch(desktop.value, (const uint8_t *)request, strlen(request));
    assert(result.status == ZMUX_OK);
    assert(zmux_buffer_release(result.value) == ZMUX_OK);
    int exited = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        char *text = desktop_snapshot(desktop.value);
        exited = strstr(text, "\"state\":\"exited\"") != NULL;
        free(text);
        if (exited) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(exited);
    /* Ensure the worker's wake reader has been dropped before cleanup wakes it. */
    nanosleep(&(struct timespec){.tv_nsec = 100000000}, NULL);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
    assert(signal(SIGPIPE, SIG_DFL) == SIG_DFL);
}

static const char *snapshot = "{\"version\":1,\"command\":{\"method\":\"snapshot\"}}";
static const char *create = "{\"version\":1,\"command\":{\"method\":\"apply\",\"params\":{\"action\":\"create_workspace\",\"name\":\"from-C\"}}}";

static ZmuxResult dispatch(uint64_t core, const char *request) {
    return zmux_core_dispatch(core, (const uint8_t *)request, strlen(request));
}
static char *consume(uint64_t buffer) {
    ZmuxResult length = zmux_buffer_length(buffer);
    assert(length.status == ZMUX_OK && length.value > 0 && length.value <= 1048576);
    char *text = malloc((size_t)length.value + 1);
    assert(text != NULL);
    assert(zmux_buffer_copy(buffer, (uint8_t *)text, (size_t)length.value) == ZMUX_OK);
    text[length.value] = '\0';
    assert(zmux_buffer_release(buffer) == ZMUX_OK);
    assert(zmux_buffer_release(buffer) == ZMUX_INVALID_HANDLE);
    assert(zmux_buffer_length(buffer).status == ZMUX_INVALID_HANDLE);
    return text;
}
static void emit(uint64_t buffer) {
    char *text = consume(buffer);
    puts(text);
    free(text);
}
static void command(uint64_t core, const char *request) {
    ZmuxResult response = dispatch(core, request);
    assert(response.status == ZMUX_OK);
    emit(response.value);
}

static void native_transfer_receipts_are_session_bound(void) {
    ZmuxResult desktop = zmux_desktop_create();
    assert(desktop.status == ZMUX_OK);
    ZmuxResult browser_batch = zmux_desktop_browser_drain(desktop.value);
    assert(browser_batch.status == ZMUX_OK);
    char *batch_text = consume(browser_batch.value);
    assert(strstr(batch_text, "\"requests\":[]") != NULL);
    free(batch_text);
    assert(zmux_desktop_browser_event(desktop.value, NULL, 1) == ZMUX_INVALID_ARGUMENT);
    const uint8_t bad_event[] = "{}";
    assert(zmux_desktop_browser_event(desktop.value, bad_event, 300001) == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_browser_event(desktop.value, bad_event, sizeof bad_event - 1) == ZMUX_INVALID_ARGUMENT);
    /* Foreign callback bytes must be rejected before parsing or executor admission. */
    assert(zmux_desktop_notification_event(desktop.value, NULL, 1) == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_notification_event(desktop.value, bad_event, 16385) == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_notification_event(desktop.value, bad_event, sizeof bad_event - 1) == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_notification_action(desktop.value, NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_notification_action(desktop.value, bad_event, 32769).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_notification_action(desktop.value, bad_event, sizeof bad_event - 1).status == ZMUX_INVALID_ARGUMENT);
    uint64_t surface = 0;
    uint64_t session = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        char *snapshot = desktop_snapshot(desktop.value);
        char *surfaces = strstr(snapshot, "\"surfaces\":[");
        char *surface_field = surfaces == NULL ? NULL : strstr(surfaces, "\"id\":");
        char *session_field = surface_field == NULL ? NULL : strstr(surface_field, "\"session_id\":");
        if (surface_field != NULL && session_field != NULL && strstr(snapshot, "\"state\":\"running\"") != NULL) {
            surface = strtoull(surface_field + strlen("\"id\":"), NULL, 10);
            session = strtoull(session_field + strlen("\"session_id\":"), NULL, 10);
        }
        free(snapshot);
        if (surface != 0 && session != 0) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(surface != 0 && session != 0);

    /* The native reply channel reserves response capacity before consuming an
     * opted-in generation, including empty-reply navigation fallbacks. */
    char notification[512];
    snprintf(notification, sizeof notification, "{\"version\":1,\"request_id\":90,\"payload\":{\"command\":\"attention\",\"action\":{\"operation\":\"notify\",\"surface_id\":%llu,\"title\":\"ABI reply capacity\",\"desktop\":false,\"reply\":true}}}", (unsigned long long)surface);
    ZmuxResult notification_admission = zmux_desktop_dispatch(desktop.value, (const uint8_t *)notification, strlen(notification));
    assert(notification_admission.status == ZMUX_OK);
    free(consume(notification_admission.value));
    char target[1024] = {0};
    for (int attempt = 0; attempt < 500 && target[0] == '\0'; attempt++) {
        char *text = desktop_snapshot(desktop.value);
        char *start = strstr(text, "\"target\":{");
        if (start != NULL) {
            start += strlen("\"target\":");
            char *end = start;
            int depth = 0;
            do { if (*end == '{') { depth++; } else if (*end == '}') { depth--; } end++; } while (*end != '\0' && depth > 0);
            assert(depth == 0 && (size_t)(end - start) < sizeof target);
            memcpy(target, start, (size_t)(end - start));
        }
        free(text);
        if (target[0] == '\0') { nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL); }
    }
    assert(target[0] != '\0');
    uint64_t held_buffers[16];
    size_t held_count = 0;
    for (;;) {
        ZmuxResult held = zmux_desktop_snapshot(desktop.value, 0);
        if (held.status == ZMUX_CAPACITY) { break; }
        if (held.status == ZMUX_BUSY) { continue; }
        assert(held.status == ZMUX_OK && held_count < 16);
        held_buffers[held_count++] = held.value;
    }
    char empty_reply[1400];
    snprintf(empty_reply, sizeof empty_reply, "{\"action\":\"reply\",\"target\":%s,\"text\":\"\"}", target);
    assert(zmux_desktop_notification_action(desktop.value, (const uint8_t *)empty_reply, strlen(empty_reply)).status == ZMUX_CAPACITY);
    for (size_t index = 0; index < held_count; index++) { assert(zmux_buffer_release(held_buffers[index]) == ZMUX_OK); }
    char *unconsumed = desktop_snapshot(desktop.value);
    assert(strstr(unconsumed, "\"reply_state\":\"ready\"") != NULL);
    assert(strstr(unconsumed, "\"read\":false") != NULL);
    free(unconsumed);

    /* Valid bounded raw text may exceed 16 KiB after JSON escaping. An invalid
     * owner reaches executor rejection, rather than being rejected as bytes. */
    char *escaped_reply = calloc(32768, 1);
    assert(escaped_reply != NULL);
    const char *prefix = "{\"action\":\"reply\",\"target\":{\"epoch\":\"00000000-0000-0000-0000-000000000000\",\"identity_namespace\":\"00000000-0000-0000-0000-000000000000\",\"notification_id\":1,\"destination\":{\"kind\":\"surface\",\"surface_id\":1,\"session_id\":1}},\"text\":\"";
    size_t escaped_length = strlen(prefix);
    memcpy(escaped_reply, prefix, escaped_length);
    for (size_t index = 0; index < 4095; index++) { memcpy(escaped_reply + escaped_length, "\\u0001", 6); escaped_length += 6; }
    memcpy(escaped_reply + escaped_length, "\"}", 2); escaped_length += 2;
    assert(escaped_length > 16384 && escaped_length < 32768);
    assert(zmux_desktop_notification_action(desktop.value, (const uint8_t *)escaped_reply, escaped_length).status == ZMUX_INTERNAL);
    free(escaped_reply);

    char path[96];
    snprintf(path, sizeof path, "/tmp/zmux-abi-transfer-%ld", (long)getpid());
    FILE *source = fopen(path, "w");
    assert(source != NULL);
    assert(fputs("native transfer receipt\n", source) >= 0);
    assert(fclose(source) == 0);

    char request[512];
    snprintf(request, sizeof request, "{\"version\":1,\"request_id\":91,\"payload\":{\"command\":\"transfer\",\"action\":{\"operation\":\"paste_files\",\"surface_id\":%llu,\"paths\":[\"%s\"]}}}", (unsigned long long)surface, path);
    ZmuxResult missing_session = zmux_desktop_transfer_files(desktop.value, (const uint8_t *)request, strlen(request));
    assert(missing_session.status == ZMUX_OK);
    char *reply = consume(missing_session.value);
    assert(strstr(reply, "\"ok\":false") != NULL);
    assert(strstr(reply, "\"outcome_unknown\":false") != NULL);
    assert(strstr(reply, "expected_session_id") != NULL);
    free(reply);

    snprintf(request, sizeof request, "{\"version\":1,\"request_id\":91,\"payload\":{\"command\":\"transfer\",\"action\":{\"operation\":\"paste_files\",\"surface_id\":%llu,\"expected_session_id\":%llu,\"paths\":[\"%s\"]}}}", (unsigned long long)surface, (unsigned long long)(session + 1), path);
    ZmuxResult stale = zmux_desktop_transfer_files(desktop.value, (const uint8_t *)request, strlen(request));
    assert(stale.status == ZMUX_OK);
    reply = consume(stale.value);
    assert(strstr(reply, "\"ok\":false") != NULL);
    assert(strstr(reply, "\"outcome_unknown\":false") != NULL);
    assert(strstr(reply, "session changed") != NULL);
    free(reply);

    char *snapshot_after_stale = desktop_snapshot(desktop.value);
    assert(strstr(snapshot_after_stale, path) == NULL);
    free(snapshot_after_stale);

    snprintf(request, sizeof request, "{\"version\":1,\"request_id\":91,\"payload\":{\"command\":\"transfer\",\"action\":{\"operation\":\"paste_files\",\"surface_id\":%llu,\"expected_session_id\":%llu,\"paths\":[\"%s\"]}}}", (unsigned long long)surface, (unsigned long long)session, path);
    ZmuxResult accepted = zmux_desktop_transfer_files(desktop.value, (const uint8_t *)request, strlen(request));
    assert(accepted.status == ZMUX_OK);
    reply = consume(accepted.value);
    assert(strstr(reply, "\"ok\":true") != NULL);
    assert(strstr(reply, "\"revision\":") != NULL);
    assert(strstr(reply, "\"upload_id\":null") != NULL);
    free(reply);
    int inserted = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        char *snapshot = desktop_snapshot(desktop.value);
        inserted = strstr(snapshot, path) != NULL;
        free(snapshot);
        if (inserted) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(inserted);
    assert(unlink(path) == 0);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
}

/* Exercise the persistent constructor through foreign ABI, not just Rust owners. */
static void persistent_desktop_uses_requested_directory(void) {
    char temporary[] = "/tmp/zmux-abi-state-XXXXXX";
    assert(mkdtemp(temporary) != NULL);
    char *directory = realpath(temporary, NULL);
    assert(directory != NULL);
    ZmuxResult desktop = zmux_desktop_create_persistent((const uint8_t *)directory, strlen(directory));
    assert(desktop.status == ZMUX_OK);
    int ready = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        char *text = desktop_snapshot(desktop.value);
        ready = strstr(text, "\"enabled\":true") != NULL &&
                strstr(text, "\"startup_pending\":false") != NULL &&
                strstr(text, "\"state\":\"running\"") != NULL;
        free(text);
        if (ready) { break; }
        nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL);
    }
    assert(ready);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
    char path[4096];
    snprintf(path, sizeof path, "%s/session.json", directory);
    struct stat saved;
    assert(stat(path, &saved) == 0 && S_ISREG(saved.st_mode));
    assert((saved.st_mode & 0777) == 0600 && saved.st_size > 0);
    DIR *entries = opendir(directory);
    assert(entries != NULL);
    struct dirent *entry;
    while ((entry = readdir(entries)) != NULL) {
        if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) { continue; }
        snprintf(path, sizeof path, "%s/%s", directory, entry->d_name);
        assert(unlink(path) == 0);
    }
    assert(closedir(entries) == 0);
    assert(rmdir(directory) == 0);
    free(directory);
}

/* Foreign pointers/byte strings must be rejected before starting a runtime. */
static void persistent_path_contract(void) {
    assert(zmux_desktop_create_persistent(NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    const uint8_t relative[] = "relative/state";
    assert(zmux_desktop_create_persistent(relative, sizeof relative - 1).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_create_persistent(relative, 0).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_create_persistent(relative, 4097).status == ZMUX_INVALID_ARGUMENT);
    const uint8_t invalid[] = {'/', 0xff};
    assert(zmux_desktop_create_persistent(invalid, sizeof invalid).status == ZMUX_INVALID_ARGUMENT);
    const uint8_t embedded_nul[] = {'/', 'a', 0, 'b'};
    assert(zmux_desktop_create_persistent(embedded_nul, sizeof embedded_nul).status == ZMUX_INVALID_ARGUMENT);
}

int main(void) {
    persistent_path_contract();
    persistent_desktop_uses_requested_directory();
    assert(zmux_abi_version() == 1);
    ZmuxResult core = zmux_core_create();
    assert(core.status == ZMUX_OK && core.value != 0);
    command(core.value, "{\"version\":2,\"command\":{\"method\":\"snapshot\"}}");
    command(core.value, "{\"version\":1,\"command\":{\"method\":\"snapshot\"},\"unexpected\":true}");
    command(core.value, create);
    command(core.value, "{\"version\":1,\"command\":{\"method\":\"apply\",\"params\":{\"action\":\"create_workspace\",\"name\":\" \"}}}");
    uint8_t invalid_utf8[] = {0xff};
    ZmuxResult invalid = zmux_core_dispatch(core.value, invalid_utf8, sizeof invalid_utf8);
    assert(invalid.status == ZMUX_OK);
    emit(invalid.value);
    assert(zmux_core_dispatch(core.value, NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    uint8_t *oversized = calloc(16385, 1);
    assert(oversized != NULL);
    assert(zmux_core_dispatch(core.value, oversized, 16385).status == ZMUX_INVALID_ARGUMENT);
    free(oversized);
    ZmuxResult reply = dispatch(core.value, snapshot);
    assert(reply.status == ZMUX_OK);
    const uint8_t original[4] = {1, 2, 3, 4};
    uint8_t tiny[4];
    memcpy(tiny, original, sizeof tiny);
    assert(zmux_buffer_copy(reply.value, tiny, sizeof tiny) == ZMUX_BUFFER_TOO_SMALL);
    assert(memcmp(tiny, original, sizeof tiny) == 0);
    assert(zmux_buffer_copy(reply.value, NULL, (size_t)zmux_buffer_length(reply.value).value) == ZMUX_INVALID_ARGUMENT);
    assert(zmux_core_destroy(reply.value) == ZMUX_INVALID_HANDLE);
    assert(zmux_buffer_release(core.value) == ZMUX_INVALID_HANDLE);
    assert(zmux_core_destroy(core.value) == ZMUX_OK);
    assert(zmux_core_destroy(core.value) == ZMUX_INVALID_HANDLE);
    assert(dispatch(core.value, snapshot).status == ZMUX_INVALID_HANDLE);
    /* A response retains its own ownership after its core is destroyed. */
    emit(reply.value);

    ZmuxResult replacement = zmux_core_create();
    assert(replacement.status == ZMUX_OK && replacement.value != core.value);
    uint64_t buffers[1024];
    size_t count = 0;
    for (;;) {
        ZmuxResult response = dispatch(replacement.value, snapshot);
        if (response.status == ZMUX_CAPACITY) { break; }
        assert(response.status == ZMUX_OK && count < 1024);
        buffers[count++] = response.value;
    }
    assert(count > 0 && count <= 16);
    /* Buffer exhaustion must reject a mutation BEFORE applying it. */
    assert(dispatch(replacement.value, create).status == ZMUX_CAPACITY);
    for (size_t i = 0; i < count; i++) { assert(zmux_buffer_release(buffers[i]) == ZMUX_OK); }
    command(replacement.value, snapshot);
    assert(zmux_core_destroy(replacement.value) == ZMUX_OK);
    native_transfer_receipts_are_session_bound();
    exited_desktop_survives_close();
    return 0;
}
