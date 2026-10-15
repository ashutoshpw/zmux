// SPDX-License-Identifier: MIT
// Original metadata callback capture; no parser, second engine or callback allocation.
#include <stdlib.h>
#include <string.h>
#include "metadata.h"
#include <ghostty/vt/zmux_metadata.h>
typedef ZmuxGhosttyMetadataV1 UpstreamMetadata;
struct Metadata {
  Integrated *integrated;
  ZmuxMetadataRecord records[ZMUX_METADATA_RECORDS];
  uint8_t bytes[ZMUX_METADATA_BYTES];
  size_t records_length, bytes_length, byte_limit, capture_limit;
  uint32_t failure;
};
static void metadata_event(void *context, const UpstreamMetadata *event) {
  Metadata *m = context;
  if (m->failure) return;
  if (!event || event->size != sizeof(*event) || event->status >= 3) {
    m->failure = event && event->size == sizeof(*event) && event->status == 3 ? 19 :
      event && event->size == sizeof(*event) && event->status == 4 ? 20 : 1;
    integrated_abort(m->integrated); return;
  }
  if (m->records_length == ZMUX_METADATA_RECORDS
      || event->payload.len > m->byte_limit - m->bytes_length) {
    m->failure = 2; integrated_abort(m->integrated); return;
  }
  size_t index = m->records_length;
  if (integrated_append(m->integrated, (ZmuxEffectEvent){ZMUX_METADATA_EVENT, (uint32_t)index, 0})) {
    m->failure = 3; integrated_abort(m->integrated); return;
  }
  m->records[m->records_length++] = (ZmuxMetadataRecord){event->source, event->subcommand,
    event->status, event->terminator, (uint32_t)m->bytes_length, (uint32_t)event->payload.len};
  if (event->payload.len) memcpy(m->bytes + m->bytes_length, event->payload.ptr, event->payload.len);
  m->bytes_length += event->payload.len;
}
static GhosttyResult create(const GhosttyAllocator *general, const GhosttyAllocator *pages,
  GhosttyTerminal *terminal, uint16_t cols, uint16_t rows, size_t history, void *context) {
  Metadata *m = context;
  GhosttyResult status = zmux_ghostty_terminal_new_metadata_bounded_v1(general, pages, terminal, cols, rows,
    history, m->capture_limit, m, metadata_event);
  if (status == GHOSTTY_SUCCESS) {
    bool enabled = true;
    if (ghostty_terminal_set(*terminal, GHOSTTY_TERMINAL_OPT_TITLE_REPORT, &enabled) != GHOSTTY_INVALID_VALUE) {
      ghostty_terminal_free(*terminal); *terminal = NULL; return GHOSTTY_INVALID_VALUE;
    }
  }
  return status;
}
Metadata *metadata_new(size_t capture, size_t bytes, size_t events, size_t deny_at) {
  if (capture < 4 || capture > 16384 || bytes > ZMUX_METADATA_BYTES || events > ZMUX_EFFECT_EVENTS) return NULL;
  Metadata *m = calloc(1, sizeof(*m));
  if (!m) return NULL;
  m->capture_limit = capture; m->byte_limit = bytes;
  size_t engine = 16 * 1024 * 1024;
  m->integrated = integrated_new_factory(8, 3, 1000,
    integrated_fixed_host_bytes() + sizeof(*m) + engine, engine, engine, deny_at,
    ZMUX_EFFECT_REPLY_BYTES, events, sizeof(*m), create, m);
  if (!m->integrated) { free(m); return NULL; }
  return m;
}
Integrated *metadata_integrated(Metadata *m) { return m ? m->integrated : NULL; }
uint32_t metadata_failure(const Metadata *m) { return m ? m->failure : 1; }
int metadata_copy(Metadata *m, ZmuxMetadataRecord *records, size_t size, size_t capacity,
  size_t *count, uint8_t *bytes, size_t byte_capacity, size_t *length) {
  if (!m || !records || !bytes || !count || !length || size != sizeof(*records)
      || capacity > ZMUX_METADATA_RECORDS || byte_capacity > ZMUX_METADATA_BYTES) return -2;
  ZmuxIntegratedStats stats;
  if (m->failure || integrated_stats(m->integrated, &stats, sizeof(stats)) || stats.failures) return -1;
  if (stats.phase != ZMUX_INTEGRATED_STAGED || capacity < m->records_length || byte_capacity < m->bytes_length) return -2;
  memcpy(records, m->records, m->records_length * sizeof(*records));
  memcpy(bytes, m->bytes, m->bytes_length);
  *count = m->records_length; *length = m->bytes_length; return 0;
}
int metadata_commit(Metadata *m, uint64_t operation) {
  if (!m || m->failure) return -1;
  int status = integrated_commit(m->integrated, operation);
  if (!status) { m->records_length = 0; m->bytes_length = 0; }
  return status;
}
void metadata_free(Metadata *m) {
  if (!m) return;
  integrated_free(m->integrated); free(m);
}
