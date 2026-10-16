/* Foreign-host contract: strict native inbox capability and reservation before mutation. */
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
#include <time.h>

static void pause_briefly(void) { nanosleep(&(struct timespec){.tv_nsec = 10000000}, NULL); }
static char *consume(ZmuxResult result) {
    assert(result.status == ZMUX_OK && result.value != 0);
    ZmuxResult length = zmux_buffer_length(result.value);
    assert(length.status == ZMUX_OK && length.value < 1048576);
    char *text = calloc((size_t)length.value + 1, 1); assert(text != NULL);
    assert(zmux_buffer_copy(result.value, (uint8_t *)text, (size_t)length.value) == ZMUX_OK);
    assert(zmux_buffer_release(result.value) == ZMUX_OK);
    return text;
}
static char *call(uint64_t desktop, const char *request) {
    for (int attempt = 0; attempt < 500; attempt++) {
        ZmuxResult result = zmux_desktop_agent_messages(desktop, (const uint8_t *)request, strlen(request));
        if (result.status == ZMUX_BUSY) { pause_briefly(); continue; }
        return consume(result);
    }
    assert(!"Native message admission timed out"); return NULL;
}
static void request(char *out, size_t capacity, const char *space, unsigned long long window, int id, const char *action) {
    int count = snprintf(out, capacity,
        "{\"origin\":{\"namespace\":\"%s\",\"window_id\":%llu},\"request_id\":\"00000000-0000-4000-8000-%012d\",\"action\":%s}",
        space, window, id, action);
    assert(count > 0 && (size_t)count < capacity);
}
static char *settled(uint64_t desktop, const char *first, const char *status) {
    char *reply = call(desktop, first);
    for (int attempt = 0; attempt < 500; attempt++) {
        if (strstr(reply, "\"state\":\"pending\"") == NULL) { return reply; }
        free(reply); pause_briefly(); reply = call(desktop, status);
    }
    assert(!"Native message worker receipt timed out"); return reply;
}
int main(int count, char **arguments) {
    assert(count == 2);
    ZmuxResult desktop = zmux_desktop_create_persistent((const uint8_t *)arguments[1], strlen(arguments[1]));
    assert(desktop.status == ZMUX_OK);
    char namespace[37] = {0}; unsigned long long window = 0;
    for (int attempt = 0; attempt < 500; attempt++) {
        ZmuxResult result = zmux_desktop_snapshot(desktop.value, 0);
        if (result.status == ZMUX_BUSY || (result.status == ZMUX_OK && result.value == 0)) { pause_briefly(); continue; }
        char *snapshot = consume(result);
        char *space = strstr(snapshot, "\"identity_namespace\":\"");
        char *windows = strstr(snapshot, "\"windows\":[{\"id\":");
        if (space != NULL && windows != NULL && strstr(snapshot, "\"startup_pending\":false") != NULL) {
            memcpy(namespace, space + strlen("\"identity_namespace\":\""), 36);
            window = strtoull(windows + strlen("\"windows\":[{\"id\":"), NULL, 10);
        }
        free(snapshot); if (window != 0) { break; } pause_briefly();
    }
    assert(window != 0 && strlen(namespace) == 36);
    char off[1024], off_status[1024], on[1024], on_status[1024], query[1024], query_status[1024];
    request(off, sizeof off, namespace, window, 1, "{\"operation\":\"settings\",\"scope\":\"global\",\"enabled\":false}");
    request(off_status, sizeof off_status, namespace, window, 1, "{\"operation\":\"status\"}");
    char *reply = settled(desktop.value, off, off_status); puts(reply); free(reply);
    request(on, sizeof on, namespace, window, 2, "{\"operation\":\"settings\",\"scope\":\"global\",\"enabled\":true}");
    request(on_status, sizeof on_status, namespace, window, 2, "{\"operation\":\"status\"}");
    uint64_t buffers[16]; size_t used = 0;
    for (;;) {
        ZmuxResult held = zmux_desktop_snapshot(desktop.value, 0);
        if (held.status == ZMUX_CAPACITY) { break; }
        if (held.status == ZMUX_BUSY) { continue; }
        assert(held.status == ZMUX_OK && held.value != 0 && used < 16); buffers[used++] = held.value;
    }
    assert(zmux_desktop_agent_messages(desktop.value, (const uint8_t *)on, strlen(on)).status == ZMUX_CAPACITY);
    for (size_t index = 0; index < used; index++) { assert(zmux_buffer_release(buffers[index]) == ZMUX_OK); }
    request(query, sizeof query, namespace, window, 3, "{\"operation\":\"settings\",\"scope\":\"global\"}");
    request(query_status, sizeof query_status, namespace, window, 3, "{\"operation\":\"status\"}");
    reply = settled(desktop.value, query, query_status); puts(reply); free(reply);
    reply = settled(desktop.value, on_status, on_status); puts(reply); free(reply);
    char stale[1024];
    request(stale, sizeof stale, "00000000-0000-0000-0000-000000000000", window, 4, "{\"operation\":\"list\"}");
    reply = call(desktop.value, stale); puts(reply); free(reply);
    request(stale, sizeof stale, namespace, window, 5, "{\"operation\":\"register\",\"provider\":\"forbidden\"}");
    assert(zmux_desktop_agent_messages(desktop.value, (const uint8_t *)stale, strlen(stale)).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_agent_messages(desktop.value, NULL, 1).status == ZMUX_INVALID_ARGUMENT);
    assert(zmux_desktop_agent_messages(desktop.value, (const uint8_t *)stale, 16385).status == ZMUX_INVALID_ARGUMENT);
    const char *generic = "{\"version\":1,\"request_id\":1,\"payload\":{\"command\":\"native_agent_messages\"}}";
    reply = consume(zmux_desktop_dispatch(desktop.value, (const uint8_t *)generic, strlen(generic)));
    assert(strstr(reply, "\"invalid_request\"") != NULL); free(reply);
    assert(zmux_desktop_destroy(desktop.value) == ZMUX_OK);
    return 0;
}
