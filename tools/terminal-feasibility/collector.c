// SPDX-License-Identifier: MIT
// Original stable, fixed callback collector shared by isolated owner fixtures.
#include <string.h>
#include "collector.h"
static int callback_ready(EffectCollector *e) {
  if (e->failure) return 0;
  if (!e->inside_call) { e->failure = ZMUX_EFFECT_ERROR_CALLBACK; return 0; }
  if (e->events_length == e->event_limit) {
    e->failure = ZMUX_EFFECT_ERROR_EVENTS; return 0;
  }
  return 1;
}
static void reply(GhosttyTerminal terminal, void *context, const uint8_t *bytes, size_t length) {
  (void)terminal;
  EffectCollector *e = context;
  if (!callback_ready(e)) return;
  if (length > e->reply_limit - e->replies_length) {
    e->failure = ZMUX_EFFECT_ERROR_REPLY; return;
  }
  e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_REPLY,
    (uint32_t)e->replies_length, (uint32_t)length};
  if (length) memcpy(e->replies + e->replies_length, bytes, length);
  e->replies_length += length;
}
int collector_append(EffectCollector *e, ZmuxEffectEvent event) {
  if (!e || !callback_ready(e)) return -1;
  e->events[e->events_length++] = event;
  return 0;
}
static void bell(GhosttyTerminal terminal, void *context) {
  (void)terminal;
  EffectCollector *e = context;
  if (callback_ready(e)) e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_BELL, 0, 0};
}
static void reset(GhosttyTerminal terminal, void *context) {
  (void)terminal;
  EffectCollector *e = context;
  if (callback_ready(e)) e->events[e->events_length++] = (ZmuxEffectEvent){ZMUX_EFFECT_RESET, 0, 0};
}

int collector_init(EffectCollector *e, size_t replies, size_t events) {
  if (!e || replies > ZMUX_EFFECT_REPLY_BYTES || events > ZMUX_EFFECT_EVENTS) return ZMUX_EFFECT_ARGUMENT;
  memset(e, 0, sizeof(*e)); e->reply_limit = replies; e->event_limit = events;
  return ZMUX_EFFECT_OK;
}
GhosttyResult collector_bind(GhosttyTerminal terminal, void *context) {
  if (!terminal || !context) return GHOSTTY_INVALID_VALUE;
  if (ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_USERDATA, context) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_WRITE_PTY, reply) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_BELL, bell) != GHOSTTY_SUCCESS
      || ghostty_terminal_set(terminal, GHOSTTY_TERMINAL_OPT_RESET, reset) != GHOSTTY_SUCCESS) return GHOSTTY_INVALID_VALUE;
  return GHOSTTY_SUCCESS;
}
int collector_begin(EffectCollector *e) {
  if (!e) return ZMUX_EFFECT_ARGUMENT;
  if (e->failure) return ZMUX_EFFECT_FATAL;
  if (e->events_length || e->replies_length) return ZMUX_EFFECT_PENDING;
  e->inside_call = 1; return ZMUX_EFFECT_OK;
}
int collector_finish(EffectCollector *e, int result) {
  e->inside_call = 0;
  if (result && !e->failure) e->failure = ZMUX_EFFECT_ERROR_ENGINE;
  return e->failure ? ZMUX_EFFECT_FATAL : ZMUX_EFFECT_OK;
}
int collector_copy(EffectCollector *e, ZmuxEffectEvent *events, size_t event_size, size_t event_capacity,
                   size_t *event_length, uint8_t *replies, size_t reply_capacity, size_t *reply_length) {
  if (!e || !events || !replies || !event_length || !reply_length
      || event_size != sizeof(*events) || event_capacity > ZMUX_EFFECT_EVENTS
      || reply_capacity > ZMUX_EFFECT_REPLY_BYTES) return ZMUX_EFFECT_ARGUMENT;
  if (e->failure) return ZMUX_EFFECT_FATAL;
  if (event_capacity < e->events_length || reply_capacity < e->replies_length) return ZMUX_EFFECT_ARGUMENT;
  memcpy(events, e->events, e->events_length * sizeof(*events));
  memcpy(replies, e->replies, e->replies_length);
  *event_length = e->events_length; *reply_length = e->replies_length;
  return ZMUX_EFFECT_OK;
}
void collector_clear(EffectCollector *e) {
  if (e && !e->failure) e->events_length = e->replies_length = 0;
}
int collector_drain(EffectCollector *e, ZmuxEffectEvent *events, size_t event_size, size_t event_capacity,
                    size_t *event_length, uint8_t *replies, size_t reply_capacity, size_t *reply_length) {
  int status = collector_copy(e, events, event_size, event_capacity, event_length, replies, reply_capacity, reply_length);
  if (!status) collector_clear(e);
  return status;
}
