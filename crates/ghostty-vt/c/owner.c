// SPDX-License-Identifier: MIT
// Promote the reviewed single-owner machinery without changing frozen evidence.
// This translation unit needs owner internals solely to export full cells before
// the existing export seal. The two fixture-local callbacks have the same name.
#include "../../../tools/terminal-feasibility/resource.c"
#define reply probe_local_reply
#include "../../../tools/terminal-feasibility/probe.c"
#undef reply
#include "../../../tools/terminal-feasibility/collector.c"
#include "../../../tools/terminal-feasibility/integrated.c"
#include "../../../tools/terminal-feasibility/metadata.c"

Metadata *zmux_vt_new(uint16_t cols, uint16_t rows, size_t history,
                      size_t engine_bytes, size_t general, size_t pages) {
  if (!cols || cols > 240 || !rows || rows > 120 || history > 1000
      || !engine_bytes || engine_bytes > 64 * 1024 * 1024
      || general > engine_bytes || pages > engine_bytes) return NULL;
  Metadata *m = calloc(1, sizeof(*m));
  if (!m) return NULL;
  m->capture_limit = 16384; m->byte_limit = ZMUX_METADATA_BYTES;
  m->integrated = integrated_new_factory(cols, rows, history,
    integrated_fixed_host_bytes() + sizeof(*m) + engine_bytes,
    general, pages, 0, ZMUX_EFFECT_REPLY_BYTES, ZMUX_EFFECT_EVENTS,
    sizeof(*m), create, m);
  if (!m->integrated) { free(m); return NULL; }
  ZmuxIntegratedStats stats;
  if (integrated_stats(m->integrated, &stats, sizeof(stats)) || stats.failures) {
    metadata_free(m); return NULL;
  }
  return m;
}

int zmux_vt_export(Metadata *m, ZmuxIntegratedFrame *batch, size_t batch_size,
  ZmuxProbeFrame *frame, size_t frame_size, ZmuxProbeCell *cells, size_t cell_size,
  size_t cell_capacity, uint8_t *text, size_t text_capacity,
  ZmuxEffectEvent *events, size_t event_size, size_t event_capacity,
  uint8_t *replies, size_t reply_capacity) {
  if (!m || !batch || batch_size != sizeof(*batch)) return -2;
  Integrated *i = m->integrated;
  if (i->phase == ZMUX_INTEGRATED_FAILED) return -1;
  if (i->phase != ZMUX_INTEGRATED_STAGED || i->exported) return -2;
  ZmuxIntegratedFrame candidate = {.operation_id = i->operation_id};
  // Render/cell iteration may allocate. Seal only after its quota recheck and
  // callback copy pass, exactly as the retained scratch-text exporter does.
  if (probe_frame(i->resource->probe, frame, frame_size, cells, cell_size,
                  cell_capacity, text, text_capacity) || fence(i, 0))
    return fence(i, ZMUX_INTEGRATED_EXPORT);
  if (collector_copy(&i->collector, events, event_size, event_capacity,
    &candidate.event_length, replies, reply_capacity, &candidate.reply_length))
    return fence(i, ZMUX_INTEGRATED_EXPORT);
  candidate.text_length = frame->text_length;
  candidate.x = frame->cursor_x; candidate.y = frame->cursor_y;
  *batch = candidate; i->exported = true; return 0;
}
