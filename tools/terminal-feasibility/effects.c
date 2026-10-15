// SPDX-License-Identifier: MIT
// Original MIT callback owner. Callbacks copy only into fixed, stable host storage.
#include <ghostty/vt.h>
#include <stdlib.h>
#include <string.h>
#include "effects.h"
typedef struct Probe Probe;
extern Probe *probe_wrap_terminal(GhosttyTerminal, const GhosttyAllocator *);
extern void probe_free(Probe *);
extern int probe_feed(Probe *, const uint8_t *, size_t);
extern int probe_resize(Probe *, uint16_t, uint16_t);
extern int probe_copy(Probe *, uint8_t *, size_t, size_t *, uint16_t *, uint16_t *);
struct Effects {
  Probe *probe;
  GhosttyTerminal terminal; // Borrowed from probe; callbacks outlive terminal free.
  ZmuxEffectEvent events[ZMUX_EFFECT_EVENTS];
  uint8_t replies[ZMUX_EFFECT_REPLY_BYTES];
  size_t events_length, replies_length, event_limit, reply_limit;
  int failure, inside_call;
};
static int callback_ready(Effects *e) {
  if (e->failure) return 0;
  if (!e->inside_call) { e->failure = ZMUX_EFFECT_ERROR_CALLBACK; return 0; }
  if (e->events_length == e->event_limit) {
    e->failure = ZMUX_EFFECT_ERROR_EVENTS; return 0;
  }
  return 1;
}
static void reply(GhosttyTerminal terminal, void *context, const uint8_t *bytes, size_t length) {
  (void)terminal;
  Effects *e = context;
  if (!callback_ready(e)) return;
  if (length > e->reply_limit - e->replies_length) {
    e->failure = ZMUX_EFFECT_ERROR_REPLY; return;
  }
  e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_REPLY,
    (uint32_t)e->replies_length, (uint32_t)length};
  if (length) memcpy(e->replies + e->replies_length, bytes, length);
  e->replies_length += length;
}
static void bell(GhosttyTerminal terminal, void *context) {
  (void)terminal;
  Effects *e = context;
  if (callback_ready(e)) e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_BELL, 0, 0};
}
static void reset(GhosttyTerminal terminal, void *context) {
  (void)terminal;
  Effects *e = context;
  if (callback_ready(e)) e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_RESET, 0, 0};
}
Effects *effects_new(size_t reply_limit, size_t event_limit) {
  if (reply_limit > ZMUX_EFFECT_REPLY_BYTES || event_limit > ZMUX_EFFECT_EVENTS) return NULL;
  Effects *e = calloc(1, sizeof(*e));
  if (!e) return NULL;
  e->reply_limit = reply_limit; e->event_limit = event_limit;
  if (ghostty_terminal_new(NULL, &e->terminal, 8, 3) != GHOSTTY_SUCCESS) { free(e); return NULL; }
  e->probe = probe_wrap_terminal(e->terminal, NULL);
  if (!e->probe) { free(e); return NULL; } // wrapper consumed terminal, even on failure.
  if (ghostty_terminal_set(e->terminal, GHOSTTY_TERMINAL_OPT_USERDATA, e) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(e->terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, reply) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(e->terminal, GHOSTTY_TERMINAL_OPT_BELL, bell) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(e->terminal, GHOSTTY_TERMINAL_OPT_RESET, reset) != GHOSTTY_SUCCESS) {
    effects_free(e); return NULL;
  }
  return e;
}
void effects_free(Effects *e) {
  if (!e) return;
  // Context and its fixed buffers stay live until all engine/render handles are freed.
  probe_free(e->probe);
  free(e);
}
int effects_error(const Effects *e) { return e ? e->failure : ZMUX_EFFECT_ERROR_ENGINE; }
static int begin(Effects *e) {
  if (!e) return ZMUX_EFFECT_ARGUMENT;
  if (e->failure) return ZMUX_EFFECT_FATAL;
  if (e->events_length || e->replies_length) return ZMUX_EFFECT_PENDING;
  e->inside_call = 1;
  return ZMUX_EFFECT_OK;
}
static int finish(Effects *e, int result) {
  e->inside_call = 0;
  if (result && !e->failure) e->failure = ZMUX_EFFECT_ERROR_ENGINE;
  return e->failure ? ZMUX_EFFECT_FATAL : ZMUX_EFFECT_OK;
}
int effects_feed(Effects *e, const uint8_t *bytes, size_t length) {
  if ((!bytes && length) || length > 4096) return ZMUX_EFFECT_ARGUMENT;
  int status = begin(e);
  return status ? status : finish(e, probe_feed(e->probe, bytes, length));
}
int effects_resize(Effects *e, uint16_t cols, uint16_t rows) {
  if (!cols || cols > 240 || !rows || rows > 120) return ZMUX_EFFECT_ARGUMENT;
  int status = begin(e);
  return status ? status : finish(e, probe_resize(e->probe, cols, rows));
}
int effects_snapshot(Effects *e, uint8_t *out, size_t capacity, size_t *length, uint16_t *x, uint16_t *y) {
  if (!e || !out || !length || !x || !y || capacity > 512 * 1024) return ZMUX_EFFECT_ARGUMENT;
  if (e->failure) return ZMUX_EFFECT_FATAL;
  if (probe_copy(e->probe, out, capacity, length, x, y)) {
    e->failure = ZMUX_EFFECT_ERROR_SNAPSHOT;
    return ZMUX_EFFECT_FATAL;
  }
  return ZMUX_EFFECT_OK;
}
int effects_drain(Effects *e, ZmuxEffectEvent *events, size_t event_size, size_t event_capacity,
                  size_t *event_length, uint8_t *replies, size_t reply_capacity, size_t *reply_length) {
  if (!e || !events || !replies || !event_length || !reply_length
      || event_size != sizeof(*events) || event_capacity > ZMUX_EFFECT_EVENTS
      || reply_capacity > ZMUX_EFFECT_REPLY_BYTES) return ZMUX_EFFECT_ARGUMENT;
  if (e->failure) return ZMUX_EFFECT_FATAL;
  if (event_capacity < e->events_length || reply_capacity < e->replies_length) return ZMUX_EFFECT_ARGUMENT;
  memcpy(events, e->events, e->events_length * sizeof(*events));
  memcpy(replies, e->replies, e->replies_length);
  *event_length = e->events_length; *reply_length = e->replies_length;
  e->events_length = e->replies_length = 0;
  return ZMUX_EFFECT_OK;
}
size_t effects_host_storage_bytes(void) { return sizeof(Effects) + 8192; }
