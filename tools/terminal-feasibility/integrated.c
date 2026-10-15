// SPDX-License-Identifier: MIT
// One bounded Resource, one stable collector, one failure/publication fence.
#include <limits.h>
#include <stdlib.h>
#include "collector.h"
#include "resource-internal.h"
#include "integrated.h"
struct Integrated {
  Resource *resource;
  EffectCollector collector;
  size_t fixed_host, managed_limit;
  uint64_t operation_id, committed_id;
  uint32_t failures, phase;
  bool exported;
};
size_t integrated_fixed_host_bytes(void) { return sizeof(Integrated) + resource_host_storage_bytes(); }
static int fence(Integrated *i, uint32_t failure) {
  ZmuxResourceStats stats = {0};
  i->failures |= failure;
  if (!i->resource || resource_stats(i->resource, &stats, sizeof(stats))) i->failures |= ZMUX_INTEGRATED_CALL;
  if (stats.denials) i->failures |= ZMUX_INTEGRATED_QUOTA;
  if (stats.invalid) i->failures |= ZMUX_INTEGRATED_INVALID;
  switch (i->collector.failure) {
    case ZMUX_EFFECT_ERROR_NONE: break;
    case ZMUX_EFFECT_ERROR_REPLY: i->failures |= ZMUX_INTEGRATED_REPLY; break;
    case ZMUX_EFFECT_ERROR_EVENTS: i->failures |= ZMUX_INTEGRATED_EVENTS; break;
    default: i->failures |= ZMUX_INTEGRATED_CALL; break;
  }
  if (i->failures) { i->phase = ZMUX_INTEGRATED_FAILED; i->exported = false; return -1; }
  return 0;
}
Integrated *integrated_new(uint16_t cols, uint16_t rows, size_t history, size_t managed,
                           size_t general, size_t pages, size_t deny_at, size_t replies, size_t events) {
  return integrated_new_factory(cols, rows, history, managed, general, pages, deny_at,
                                replies, events, 0, NULL, NULL);
}
Integrated *integrated_new_factory(uint16_t cols, uint16_t rows, size_t history, size_t managed,
  size_t general, size_t pages, size_t deny_at, size_t replies, size_t events,
  size_t extra_fixed, ResourceCreateFn create, void *context) {
  if (extra_fixed > SIZE_MAX - integrated_fixed_host_bytes()) return NULL;
  size_t fixed = integrated_fixed_host_bytes() + extra_fixed;
  if (managed < fixed || managed - fixed > 64 * 1024 * 1024
      || general > managed - fixed || pages > managed - fixed
      || replies > ZMUX_EFFECT_REPLY_BYTES || events > ZMUX_EFFECT_EVENTS) return NULL;
  Integrated *i = calloc(1, sizeof(*i));
  if (!i) return NULL;
  i->fixed_host = fixed; i->managed_limit = managed;
  collector_init(&i->collector, replies, events);
  i->resource = resource_new_factory(cols, rows, history, managed - fixed, general, pages, deny_at,
                                   1, create, context);
  if (!i->resource) { free(i); return NULL; }
  if (!resource_ready(i->resource) || resource_bind_callbacks(i->resource, collector_bind, &i->collector)) fence(i, ZMUX_INTEGRATED_CALL);
  return i; // Keep failed construction observable through stats/close.
}
int integrated_append(Integrated *i, ZmuxEffectEvent event) {
  if (!i || !i->collector.inside_call || i->exported || i->phase == ZMUX_INTEGRATED_CLOSED) return -2;
  return collector_append(&i->collector, event);
}
int integrated_stats(Integrated *i, ZmuxIntegratedStats *out, size_t size) {
  if (!i || !out || size != sizeof(*out)) return -2;
  ZmuxResourceStats stats = {0};
  if (resource_stats(i->resource, &stats, sizeof(stats))) return -2;
  *out = (ZmuxIntegratedStats){stats, i->fixed_host, i->managed_limit,
    i->fixed_host + stats.reserved, i->fixed_host + stats.peak,
    i->operation_id, i->committed_id, i->failures, i->phase};
  return 0;
}
int integrated_limits(Integrated *i, size_t managed, size_t general, size_t pages) {
  if (!i || i->phase == ZMUX_INTEGRATED_FAILED || i->phase == ZMUX_INTEGRATED_CLOSED
      || (i->phase == ZMUX_INTEGRATED_STAGED && i->exported) || managed < i->fixed_host) return -2;
  if (resource_limits(i->resource, managed - i->fixed_host, general, pages)) return -2;
  i->managed_limit = managed; return 0;
}
static int begin(Integrated *i) {
  if (!i || i->phase == ZMUX_INTEGRATED_CLOSED) return -2;
  if (i->phase == ZMUX_INTEGRATED_FAILED || fence(i, 0)) return -1;
  if (i->phase != ZMUX_INTEGRATED_IDLE) return -3;
  if (i->operation_id == UINT64_MAX) return fence(i, ZMUX_INTEGRATED_CALL);
  if (collector_begin(&i->collector)) return fence(i, ZMUX_INTEGRATED_CALL);
  i->operation_id++; i->exported = false; return 0;
}
static int finish(Integrated *i, int result) {
  collector_finish(&i->collector, 0);
  if (fence(i, 0)) return -1;
  if (result) return fence(i, ZMUX_INTEGRATED_CALL);
  i->phase = ZMUX_INTEGRATED_STAGED; return 0;
}
int integrated_feed(Integrated *i, const uint8_t *bytes, size_t length) {
  if ((!bytes && length) || length > 4096) return -2;
  int status = begin(i); return status ? status : finish(i, resource_feed(i->resource, bytes, length));
}
int integrated_resize(Integrated *i, uint16_t cols, uint16_t rows) {
  if (!cols || cols > 240 || !rows || rows > 120) return -2;
  int status = begin(i); return status ? status : finish(i, resource_resize(i->resource, cols, rows));
}
int integrated_scroll(Integrated *i, int32_t lines) {
  if (lines < -1000 || lines > 1000) return -2;
  int status = begin(i); return status ? status : finish(i, resource_scroll(i->resource, lines));
}
int integrated_clone(Integrated *i, size_t *history) {
  if (!history) return -2;
  int status = begin(i); return status ? status : finish(i, resource_clone(i->resource, history));
}
int integrated_history(Integrated *i, size_t *history) {
  if (!i || !history || i->phase != ZMUX_INTEGRATED_IDLE) return -2;
  int result = resource_history(i->resource, history);
  return fence(i, result ? ZMUX_INTEGRATED_CALL : 0);
}
int integrated_export(Integrated *i, ZmuxIntegratedFrame *frame, size_t frame_size,
                       uint8_t *text, size_t text_capacity, ZmuxEffectEvent *events,
                       size_t event_size, size_t event_capacity, uint8_t *replies, size_t reply_capacity) {
  if (!i || !frame || !text || !events || !replies || frame_size != sizeof(*frame)
      || text_capacity != 512 * 1024 || event_size != sizeof(*events)
      || event_capacity > ZMUX_EFFECT_EVENTS || reply_capacity > ZMUX_EFFECT_REPLY_BYTES) return -2;
  if (i->phase == ZMUX_INTEGRATED_FAILED) return -1;
  if (i->phase != ZMUX_INTEGRATED_STAGED) return -2;
  if (i->exported) return -3; // Successful export seals all engine/config access until commit.
  if (event_capacity < i->collector.events_length || reply_capacity < i->collector.replies_length) return -2;
  ZmuxIntegratedFrame candidate = {.operation_id = i->operation_id};
  int snapshot_status = resource_snapshot(i->resource, text, text_capacity, &candidate.text_length, &candidate.x, &candidate.y);
  // Render/cell iteration may allocate or fail after a healthy feed. Never retire
  // callbacks or authorize commit until both the export result and quota recheck pass.
  if (snapshot_status || fence(i, 0)) return fence(i, ZMUX_INTEGRATED_EXPORT);
  if (collector_copy(&i->collector, events, event_size, event_capacity, &candidate.event_length,
                     replies, reply_capacity, &candidate.reply_length)) return fence(i, ZMUX_INTEGRATED_EXPORT);
  *frame = candidate; i->exported = true; return 0;
}
int integrated_commit(Integrated *i, uint64_t operation) {
  if (!i || i->phase != ZMUX_INTEGRATED_STAGED || !i->exported || operation != i->operation_id) return -2;
  // No engine operation or allocation occurs after validated export. All mutations
  // are fenced by STAGED, so retiring this exact batch cannot acquire a new failure.
  collector_clear(&i->collector); i->committed_id = operation;
  i->exported = false; i->phase = ZMUX_INTEGRATED_IDLE; return 0;
}
void integrated_abort(Integrated *i) { if (i && i->phase != ZMUX_INTEGRATED_CLOSED) fence(i, ZMUX_INTEGRATED_HOST); }
void integrated_close(Integrated *i) {
  if (!i || i->phase == ZMUX_INTEGRATED_CLOSED) return;
  resource_close(i->resource); i->phase = ZMUX_INTEGRATED_CLOSED; i->exported = false;
}
void integrated_free(Integrated *i) {
  if (!i) return;
  integrated_close(i); resource_free(i->resource); free(i);
}
