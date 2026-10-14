/* Independent C caller: exercises the actual shared-library boundary and owned buffers. */
#define _POSIX_C_SOURCE 200809L
#include "zmux.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <time.h>

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
static void emit(uint64_t buffer) {
    ZmuxResult length = zmux_buffer_length(buffer);
    assert(length.status == ZMUX_OK && length.value > 0 && length.value <= 1048576);
    char *text = malloc((size_t)length.value + 1);
    assert(text != NULL);
    assert(zmux_buffer_copy(buffer, (uint8_t *)text, (size_t)length.value) == ZMUX_OK);
    text[length.value] = '\0';
    puts(text);
    free(text);
    assert(zmux_buffer_release(buffer) == ZMUX_OK);
    assert(zmux_buffer_release(buffer) == ZMUX_INVALID_HANDLE);
    assert(zmux_buffer_length(buffer).status == ZMUX_INVALID_HANDLE);
}
static void command(uint64_t core, const char *request) {
    ZmuxResult response = dispatch(core, request);
    assert(response.status == ZMUX_OK);
    emit(response.value);
}

int main(void) {
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
    exited_desktop_survives_close();
    return 0;
}
