/* Independent C caller: exercises the actual shared-library boundary and owned buffers. */
#include "zmux.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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
    return 0;
}
