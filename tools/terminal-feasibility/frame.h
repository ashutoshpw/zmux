// SPDX-License-Identifier: MIT
// Original isolated Zmux frame interchange. No upstream layouts cross this ABI.
#ifndef ZMUX_FEASIBILITY_FRAME_H
#define ZMUX_FEASIBILITY_FRAME_H
#include <stddef.h>
#include <stdint.h>

typedef struct {
  uint8_t kind; // 0 default, 1 indexed, 2 RGB
  uint8_t index, r, g, b;
} ZmuxProbeColor;
typedef struct {
  uint32_t text_offset, text_length;
  uint16_t row, column;
  uint8_t width;
  ZmuxProbeColor foreground, background;
  uint8_t bold, italic, underline, inverse;
} ZmuxProbeCell;
typedef struct {
  uint32_t version, unsupported;
  uint64_t history_rows, scroll_total, scroll_offset, scroll_len;
  size_t cells_length, text_length;
  uint16_t rows, cols, cursor_x, cursor_y;
  uint8_t pending_wrap, cursor_visible, application_cursor, bracketed_paste, alternate;
} ZmuxProbeFrame;
enum {
  ZMUX_PROBE_EXTRA_STYLE = 1,
  ZMUX_PROBE_HYPERLINK = 2,
  ZMUX_PROBE_COLOR_OVERRIDE = 4,
  ZMUX_PROBE_METADATA = 8
};
#endif
