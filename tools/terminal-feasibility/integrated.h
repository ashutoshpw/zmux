// SPDX-License-Identifier: MIT
// Original single-owner experiment ABI; no application or upstream private layouts.
#ifndef ZMUX_FEASIBILITY_INTEGRATED_H
#define ZMUX_FEASIBILITY_INTEGRATED_H
#include "effects.h"
#include "resource.h"
enum { ZMUX_INTEGRATED_IDLE = 0, ZMUX_INTEGRATED_STAGED = 1, ZMUX_INTEGRATED_FAILED = 2, ZMUX_INTEGRATED_CLOSED = 3 };
enum {
  ZMUX_INTEGRATED_QUOTA = 1, ZMUX_INTEGRATED_INVALID = 2,
  ZMUX_INTEGRATED_REPLY = 4, ZMUX_INTEGRATED_EVENTS = 8,
  ZMUX_INTEGRATED_CALL = 16, ZMUX_INTEGRATED_EXPORT = 32,
  ZMUX_INTEGRATED_HOST = 64
};
typedef struct {
  ZmuxResourceStats engine;
  uint64_t fixed_host, managed_limit, managed_current, managed_peak;
  uint64_t operation_id, committed_id;
  uint32_t failures, phase;
} ZmuxIntegratedStats;
typedef struct {
  uint64_t operation_id;
  size_t text_length, event_length, reply_length;
  uint16_t x, y;
} ZmuxIntegratedFrame;
typedef struct Integrated Integrated;
size_t integrated_fixed_host_bytes(void);
Integrated *integrated_new(uint16_t, uint16_t, size_t, size_t, size_t, size_t, size_t, size_t, size_t);
int integrated_stats(Integrated *, ZmuxIntegratedStats *, size_t);
// Tightening changes checked scalar caps only; allowed before validated export,
// never between successful export and commit. History queries require IDLE.
int integrated_limits(Integrated *, size_t, size_t, size_t);
int integrated_feed(Integrated *, const uint8_t *, size_t);
int integrated_resize(Integrated *, uint16_t, uint16_t);
int integrated_scroll(Integrated *, int32_t);
int integrated_history(Integrated *, size_t *);
int integrated_clone(Integrated *, size_t *);
// Copy stages into caller-owned candidate storage; no batch retirement/publication.
int integrated_export(Integrated *, ZmuxIntegratedFrame *, size_t, uint8_t *, size_t,
                      ZmuxEffectEvent *, size_t, size_t, uint8_t *, size_t);
// Commit is nonallocating. It requires the same staged ID and a successful export.
int integrated_commit(Integrated *, uint64_t);
void integrated_abort(Integrated *);
void integrated_close(Integrated *);
void integrated_free(Integrated *);
#endif
