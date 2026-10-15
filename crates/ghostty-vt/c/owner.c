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

// Encoding reads only the authoritative idle terminal. No staged state, callback
// output or physical write may cross this boundary. All encoder allocations use
// the same bounded resource allocator and failure fence as terminal mutations.
static int input_ready(Metadata *m) {
  if (!m) return -2;
  Integrated *i = m->integrated;
  if (fence(i, 0)) return -1;
  return i->phase == ZMUX_INTEGRATED_IDLE ? 0 : -3;
}

int zmux_vt_encode_key(Metadata *m, uint32_t key, uint8_t modifiers,
  uint8_t action, uint8_t *out, size_t capacity, size_t *length) {
  // Original host IDs, matching terminal-adapter Key, not upstream ordinals.
  static const GhosttyKey keys[] = {GHOSTTY_KEY_UNIDENTIFIED,
    GHOSTTY_KEY_ARROW_LEFT, GHOSTTY_KEY_ARROW_RIGHT, GHOSTTY_KEY_ARROW_DOWN,
    GHOSTTY_KEY_ARROW_UP, GHOSTTY_KEY_HOME, GHOSTTY_KEY_END, GHOSTTY_KEY_DELETE,
    GHOSTTY_KEY_PAGE_UP, GHOSTTY_KEY_PAGE_DOWN, GHOSTTY_KEY_F1, GHOSTTY_KEY_F2,
    GHOSTTY_KEY_F3, GHOSTTY_KEY_F4, GHOSTTY_KEY_F5, GHOSTTY_KEY_F6,
    GHOSTTY_KEY_F7, GHOSTTY_KEY_F8, GHOSTTY_KEY_F9, GHOSTTY_KEY_F10,
    GHOSTTY_KEY_F11, GHOSTTY_KEY_F12, GHOSTTY_KEY_ENTER, GHOSTTY_KEY_NUMPAD_ENTER,
    GHOSTTY_KEY_BACKSPACE, GHOSTTY_KEY_ESCAPE, GHOSTTY_KEY_TAB};
  if (!key || key >= sizeof(keys)/sizeof(*keys) || (modifiers & ~7)
      || action > 2 || !out || capacity != 128 || !length) return -2;
  int ready = input_ready(m); if (ready) return ready;
  Integrated *i = m->integrated;
  GhosttyKeyEncoder encoder = NULL;
  GhosttyKeyEvent event = NULL;
  int failed = ghostty_key_encoder_new(&i->resource->general_allocator, &encoder)
    != GHOSTTY_SUCCESS;
  if (!failed) failed = ghostty_key_event_new(&i->resource->general_allocator,
    &event) != GHOSTTY_SUCCESS;
  if (!failed) {
    ghostty_key_encoder_setopt_from_terminal(encoder, i->resource->terminal);
    GhosttyOptionAsAlt option = GHOSTTY_OPTION_AS_ALT_TRUE;
    ghostty_key_encoder_setopt(encoder, GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT, &option);
    GhosttyMods mods = ((modifiers & 1) ? GHOSTTY_MODS_SHIFT : 0)
      | ((modifiers & 2) ? GHOSTTY_MODS_ALT : 0)
      | ((modifiers & 4) ? GHOSTTY_MODS_CTRL : 0);
    ghostty_key_event_set_action(event, (GhosttyKeyAction)action);
    ghostty_key_event_set_key(event, keys[key]);
    ghostty_key_event_set_mods(event, mods);
    failed = ghostty_key_encoder_encode(encoder, event, (char *)out, capacity,
      length) != GHOSTTY_SUCCESS || *length > capacity;
  }
  ghostty_key_event_free(event);
  ghostty_key_encoder_free(encoder);
  return fence(i, failed ? ZMUX_INTEGRATED_CALL : 0);
}

int zmux_vt_encode_paste(Metadata *m, uint8_t *text, size_t length,
  uint8_t *out, size_t capacity, size_t *written) {
  if (!text || !length || length > 16384 || !out || capacity != 16396
      || !written) return -2;
  int ready = input_ready(m); if (ready) return ready;
  Integrated *i = m->integrated;
  GhosttyTerminalModeConfig mode = {.mode = GHOSTTY_MODE_BRACKETED_PASTE};
  int failed = ghostty_terminal_get(i->resource->terminal,
    GHOSTTY_TERMINAL_DATA_MODE, &mode) != GHOSTTY_SUCCESS;
  if (!failed) failed = ghostty_paste_encode((char *)text, length, mode.value,
    (char *)out, capacity, written) != GHOSTTY_SUCCESS || *written > capacity;
  return fence(i, failed ? ZMUX_INTEGRATED_CALL : 0);
}

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
