// SPDX-License-Identifier: MIT
// Original MIT callback owner. Callbacks copy only into fixed, stable host storage.
#include <ghostty/vt.h>
#include <stdlib.h>
#include <string.h>
#include "collector.h"
typedef struct Probe Probe;
extern Probe *probe_wrap_terminal(GhosttyTerminal, const GhosttyAllocator *);
extern void probe_free(Probe *);
extern int probe_feed(Probe *, const uint8_t *, size_t);
extern int probe_resize(Probe *, uint16_t, uint16_t);
extern int probe_copy(Probe *, uint8_t *, size_t, size_t *, uint16_t *, uint16_t *);
struct Effects {
  Probe *probe;
  GhosttyTerminal terminal; // Borrowed from probe; callbacks outlive terminal free.
  EffectCollector collector;
};
Effects *effects_new(size_t reply_limit, size_t event_limit) {
  if (reply_limit > ZMUX_EFFECT_REPLY_BYTES || event_limit > ZMUX_EFFECT_EVENTS) return NULL;
  Effects *e = calloc(1, sizeof(*e));
  if (!e) return NULL;
  collector_init(&e->collector, reply_limit, event_limit);
  if (ghostty_terminal_new(NULL, &e->terminal, 8, 3) != GHOSTTY_SUCCESS) { free(e); return NULL; }
  e->probe = probe_wrap_terminal(e->terminal, NULL);
  if (!e->probe) { free(e); return NULL; } // wrapper consumed terminal, even on failure.
  if (collector_bind(e->terminal, &e->collector) != GHOSTTY_SUCCESS) {
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
int effects_error(const Effects *e) { return e ? e->collector.failure : ZMUX_EFFECT_ERROR_ENGINE; }
int effects_feed(Effects *e, const uint8_t *bytes, size_t length) {
  if ((!bytes && length) || length > 4096) return ZMUX_EFFECT_ARGUMENT;
  int status = e ? collector_begin(&e->collector) : ZMUX_EFFECT_ARGUMENT;
  return status ? status : collector_finish(&e->collector, probe_feed(e->probe, bytes, length));
}
int effects_resize(Effects *e, uint16_t cols, uint16_t rows) {
  if (!cols || cols > 240 || !rows || rows > 120) return ZMUX_EFFECT_ARGUMENT;
  int status = e ? collector_begin(&e->collector) : ZMUX_EFFECT_ARGUMENT;
  return status ? status : collector_finish(&e->collector, probe_resize(e->probe, cols, rows));
}
int effects_snapshot(Effects *e, uint8_t *out, size_t capacity, size_t *length, uint16_t *x, uint16_t *y) {
  if (!e || !out || !length || !x || !y || capacity > 512 * 1024) return ZMUX_EFFECT_ARGUMENT;
  if (e->collector.failure) return ZMUX_EFFECT_FATAL;
  if (probe_copy(e->probe, out, capacity, length, x, y)) {
    e->collector.failure = ZMUX_EFFECT_ERROR_SNAPSHOT;
    return ZMUX_EFFECT_FATAL;
  }
  return ZMUX_EFFECT_OK;
}
int effects_drain(Effects *e, ZmuxEffectEvent *events, size_t event_size, size_t event_capacity,
                  size_t *event_length, uint8_t *replies, size_t reply_capacity, size_t *reply_length) {
  return e ? collector_drain(&e->collector, events, event_size, event_capacity, event_length,
                             replies, reply_capacity, reply_length) : ZMUX_EFFECT_ARGUMENT;
}
size_t effects_host_storage_bytes(void) { return sizeof(Effects) + 8192; }
