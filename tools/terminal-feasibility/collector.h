// SPDX-License-Identifier: MIT
// Original fixed callback collector; owns no terminal, parser or heap allocation.
#ifndef ZMUX_FEASIBILITY_COLLECTOR_H
#define ZMUX_FEASIBILITY_COLLECTOR_H
#include <ghostty/vt.h>
#include "effects.h"
typedef struct {
  ZmuxEffectEvent events[ZMUX_EFFECT_EVENTS];
  uint8_t replies[ZMUX_EFFECT_REPLY_BYTES];
  size_t events_length, replies_length, event_limit, reply_limit;
  int failure, inside_call;
} EffectCollector;
int collector_init(EffectCollector *, size_t, size_t);
GhosttyResult collector_bind(GhosttyTerminal, void *);
int collector_begin(EffectCollector *);
int collector_finish(EffectCollector *, int);
int collector_copy(EffectCollector *, ZmuxEffectEvent *, size_t, size_t, size_t *, uint8_t *, size_t, size_t *);
int collector_drain(EffectCollector *, ZmuxEffectEvent *, size_t, size_t, size_t *, uint8_t *, size_t, size_t *);
void collector_clear(EffectCollector *);
int collector_append(EffectCollector *, ZmuxEffectEvent);
#endif
