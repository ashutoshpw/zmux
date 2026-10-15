// SPDX-License-Identifier: MIT
// Original isolated callback interchange. No borrowed engine data crosses this ABI.
#ifndef ZMUX_FEASIBILITY_EFFECTS_H
#define ZMUX_FEASIBILITY_EFFECTS_H
#include <stddef.h>
#include <stdint.h>
enum { ZMUX_EFFECT_BELL = 1, ZMUX_EFFECT_REPLY = 2, ZMUX_EFFECT_RESET = 3 };
enum { ZMUX_EFFECT_REPLY_BYTES = 4096, ZMUX_EFFECT_EVENTS = 1024 };
enum {
  ZMUX_EFFECT_OK = 0, ZMUX_EFFECT_FATAL = -1,
  ZMUX_EFFECT_ARGUMENT = -2, ZMUX_EFFECT_PENDING = -3
};
enum {
  ZMUX_EFFECT_ERROR_NONE = 0, ZMUX_EFFECT_ERROR_REPLY = 1,
  ZMUX_EFFECT_ERROR_EVENTS = 2, ZMUX_EFFECT_ERROR_ENGINE = 3,
  ZMUX_EFFECT_ERROR_SNAPSHOT = 4, ZMUX_EFFECT_ERROR_CALLBACK = 5
};
typedef struct { uint32_t kind, offset, length; } ZmuxEffectEvent;
// The owner is single-threaded. Callbacks run only inside feed/resize. A pending
// successful batch must be drained before another feed/resize. Fatal errors latch;
// the remainder of that feed may still execute upstream, but later calls refuse.
// Caller buffers remain owned by the caller; drain checks all capacities first.
typedef struct Effects Effects;
Effects *effects_new(size_t reply_limit, size_t event_limit);
void effects_free(Effects *);
int effects_error(const Effects *);
int effects_feed(Effects *, const uint8_t *, size_t);
int effects_resize(Effects *, uint16_t, uint16_t);
int effects_snapshot(Effects *, uint8_t *, size_t, size_t *, uint16_t *, uint16_t *);
int effects_drain(Effects *, ZmuxEffectEvent *, size_t, size_t, size_t *, uint8_t *, size_t, size_t *);
size_t effects_host_storage_bytes(void);
#endif
