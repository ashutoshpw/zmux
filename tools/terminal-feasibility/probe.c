// SPDX-License-Identifier: MIT
// Original MIT feasibility fixture. Public upstream API only.
#include <ghostty/vt.h>
#include <stdlib.h>
#include <string.h>
#include "frame.h"
typedef struct Probe {
 GhosttyTerminal terminal;
 GhosttyRenderState render;
 GhosttyRenderStateRowIterator rows;
 GhosttyRenderStateRowCells cells;
 unsigned char replies[4096]; size_t reply_len; int failed;
} Probe;
_Static_assert(sizeof(Probe) <= 8192, "resource fixture host owner bound");
static void reply(GhosttyTerminal t, void *user, const uint8_t *data, size_t n) {
 (void)t; Probe *p=user;
 if(n>sizeof(p->replies)-p->reply_len){p->failed=1;return;}
 memcpy(p->replies+p->reply_len,data,n);p->reply_len+=n;
}
void probe_free(Probe *p) {
 if(!p)return;
 if(p->cells)ghostty_render_state_row_cells_free(p->cells);
 if(p->rows)ghostty_render_state_row_iterator_free(p->rows);
 if(p->render)ghostty_render_state_free(p->render);
 if(p->terminal)ghostty_terminal_free(p->terminal);
 free(p);
}
// Take ownership of an existing terminal; allocator contexts outlive all handles.
Probe *probe_wrap_terminal(GhosttyTerminal terminal,const GhosttyAllocator *allocator) {
 if(!terminal)return NULL;
 Probe *p=calloc(1,sizeof(*p));if(!p){ghostty_terminal_free(terminal);return NULL;}
 p->terminal=terminal;
 #define CHECK(call) if((call)!=GHOSTTY_SUCCESS){probe_free(p);return NULL;}
 CHECK(ghostty_render_state_new(allocator,&p->render));
 CHECK(ghostty_render_state_row_iterator_new(allocator,&p->rows));
 CHECK(ghostty_render_state_row_cells_new(allocator,&p->cells));
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_USERDATA,p));
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_WRITE_PTY,reply));
 #undef CHECK
 return p;
}
Probe *probe_new(uint16_t cols,uint16_t rows) {
 if(!cols||!rows||cols>240||rows>120)return NULL;
 GhosttyTerminal terminal=NULL;
 if(ghostty_terminal_new(NULL,&terminal,cols,rows)!=GHOSTTY_SUCCESS)return NULL;
 Probe *p=probe_wrap_terminal(terminal,NULL);if(!p)return NULL;
 size_t history_bytes=4*1024*1024,history_lines=1000;
 if(ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,&history_bytes)!=GHOSTTY_SUCCESS
  ||ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,&history_lines)!=GHOSTTY_SUCCESS){probe_free(p);return NULL;}
 return p;
}
int probe_feed(Probe *p,const uint8_t *bytes,size_t n) {
 if(n>16384)return -1;
 ghostty_terminal_vt_write(p->terminal,bytes,n);
 bool failed=false;
 if(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR,&failed)!=GHOSTTY_SUCCESS)return -1;
 return p->failed||failed?-1:0;
}
int probe_resize(Probe *p,uint16_t cols,uint16_t rows) {
 if(!cols||!rows||cols>240||rows>120)return -1;
 return ghostty_terminal_resize(p->terminal,cols,rows,8,16)==GHOSTTY_SUCCESS?0:-1;
}
int probe_copy(Probe *p,uint8_t *out,size_t cap,size_t *length,uint16_t *x,uint16_t *y) {
 #define CHECK(call) if((call)!=GHOSTTY_SUCCESS)return -1
 CHECK(ghostty_render_state_update(p->render,p->terminal));
 CHECK(ghostty_render_state_get(p->render,GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,&p->rows));
 CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_X,x));
 CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_Y,y));
 size_t used=0;unsigned count=0;
 while(ghostty_render_state_row_iterator_next(p->rows)) {
  if(++count>120)return -1;
  CHECK(ghostty_render_state_row_get(p->rows,GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,&p->cells));
  unsigned columns=0;
  while(ghostty_render_state_row_cells_next(p->cells)) {
   if(++columns>240)return -1;
   uint8_t bytes[256];GhosttyBuffer buffer={.ptr=bytes,.cap=sizeof(bytes),.len=0};
   CHECK(ghostty_render_state_row_cells_get(p->cells,GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,&buffer));
   size_t n=buffer.len?buffer.len:1;if(n>cap-used)return -1;
   if(buffer.len)memcpy(out+used,bytes,n);else out[used]=' ';used+=n;
  }
  if(used==cap)return -1;out[used++]='\n';
 }
 *length=used;return 0;
 #undef CHECK
}
int probe_replies(Probe *p,uint8_t *out,size_t cap,size_t *n) {
 if(p->reply_len>cap)return -1;memcpy(out,p->replies,p->reply_len);*n=p->reply_len;p->reply_len=0;return p->failed?-1:0;
}

// Caller owns all output storage. Snapshot failure never publishes partial data.
static int frame_color(GhosttyStyleColor source, ZmuxProbeColor *out) {
 memset(out,0,sizeof(*out));
 switch(source.tag) {
 case GHOSTTY_STYLE_COLOR_NONE: return 0;
 case GHOSTTY_STYLE_COLOR_PALETTE: out->kind=1;out->index=source.value.palette;return 0;
 case GHOSTTY_STYLE_COLOR_RGB:
  out->kind=2;out->r=source.value.rgb.r;out->g=source.value.rgb.g;out->b=source.value.rgb.b;return 0;
 default:return -1;
 }
}
int probe_scroll(Probe *p,int32_t lines) {
 if(!p||lines < -1000||lines > 1000)return -1;
 GhosttyTerminalScrollViewport value={0};
 value.tag=lines?GHOSTTY_SCROLL_VIEWPORT_DELTA:GHOSTTY_SCROLL_VIEWPORT_BOTTOM;
 value.value.delta=-(intptr_t)lines;
 ghostty_terminal_scroll_viewport(p->terminal,value);return 0;
}
int probe_frame(Probe *p,ZmuxProbeFrame *frame,size_t frame_size,
 ZmuxProbeCell *out,size_t cell_size,size_t cells_cap,uint8_t *text,size_t text_cap) {
 if(!p||!frame||!out||!text||frame_size!=sizeof(*frame)||cell_size!=sizeof(*out)
   ||cells_cap>120*240||text_cap>512*1024)return -2;
 memset(frame,0,sizeof(*frame));frame->version=1;
 #define FRAME_CHECK(call) if((call)!=GHOSTTY_SUCCESS)return -1
 FRAME_CHECK(ghostty_render_state_update(p->render,p->terminal));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_ROWS,&frame->rows));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_COLS,&frame->cols));
 if(!frame->rows||!frame->cols||frame->rows>120||frame->cols>240)return -1;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_X,&frame->cursor_x));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_Y,&frame->cursor_y));
 bool pending=false,visible=false;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP,&pending));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,&visible));
 frame->pending_wrap=pending;frame->cursor_visible=visible;
 GhosttyTerminalModeConfig application={.mode=GHOSTTY_MODE_DECCKM};
 GhosttyTerminalModeConfig paste={.mode=GHOSTTY_MODE_BRACKETED_PASTE};
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_MODE,&application));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_MODE,&paste));
 frame->application_cursor=application.value;frame->bracketed_paste=paste.value;
 GhosttyTerminalScreen screen;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,&screen));
 if(screen!=GHOSTTY_TERMINAL_SCREEN_PRIMARY&&screen!=GHOSTTY_TERMINAL_SCREEN_ALTERNATE)return -1;
 frame->alternate=screen==GHOSTTY_TERMINAL_SCREEN_ALTERNATE;
 size_t history;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,&history));
 frame->history_rows=history;
 GhosttyTerminalScrollbar scrollbar;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_SCROLLBAR,&scrollbar));
 frame->scroll_total=scrollbar.total;frame->scroll_offset=scrollbar.offset;frame->scroll_len=scrollbar.len;
 GhosttyString title,pwd;
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_TITLE,&title));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_PWD,&pwd));
 if(title.len||pwd.len)frame->unsupported|=ZMUX_PROBE_METADATA;
 GhosttyColorRgb colors[256],defaults[256];
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_COLOR_PALETTE,colors));
 FRAME_CHECK(ghostty_terminal_get(p->terminal,GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT,defaults));
 if(memcmp(colors,defaults,sizeof(colors)))frame->unsupported|=ZMUX_PROBE_COLOR_OVERRIDE;
 const GhosttyTerminalData effective[]={GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND,
  GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND,GHOSTTY_TERMINAL_DATA_COLOR_CURSOR};
 const GhosttyTerminalData base[]={GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND_DEFAULT,
  GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT,GHOSTTY_TERMINAL_DATA_COLOR_CURSOR_DEFAULT};
 for(size_t i=0;i<3;i++) {
  GhosttyColorRgb color,def;
  GhosttyResult a=ghostty_terminal_get(p->terminal,effective[i],&color);
  GhosttyResult b=ghostty_terminal_get(p->terminal,base[i],&def);
  if((a!=GHOSTTY_SUCCESS&&a!=GHOSTTY_NO_VALUE)||(b!=GHOSTTY_SUCCESS&&b!=GHOSTTY_NO_VALUE))return -1;
  if(a!=b||(a==GHOSTTY_SUCCESS&&memcmp(&color,&def,sizeof(color))))frame->unsupported|=ZMUX_PROBE_COLOR_OVERRIDE;
 }
 FRAME_CHECK(ghostty_render_state_get(p->render,GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,&p->rows));
 size_t used=0,count=0;uint16_t row=0;
 while(ghostty_render_state_row_iterator_next(p->rows)) {
  if(row>=frame->rows)return -1;
  FRAME_CHECK(ghostty_render_state_row_get(p->rows,GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,&p->cells));
  uint16_t column=0;
  while(ghostty_render_state_row_cells_next(p->cells)) {
   if(column>=frame->cols)return -1;
   GhosttyCell raw;GhosttyCellWide wide;GhosttyCellContentTag content;bool link=false;
   FRAME_CHECK(ghostty_render_state_row_cells_get(p->cells,GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,&raw));
   FRAME_CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_WIDE,&wide));
   uint16_t current_column=column++;
   if(wide==GHOSTTY_CELL_WIDE_SPACER_TAIL)continue;
   if(wide!=GHOSTTY_CELL_WIDE_NARROW&&wide!=GHOSTTY_CELL_WIDE_WIDE
     &&wide!=GHOSTTY_CELL_WIDE_SPACER_HEAD)return -1;
   if(count>=cells_cap)return -2;
   FRAME_CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_CONTENT_TAG,&content));
   FRAME_CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_HAS_HYPERLINK,&link));
   if(link)frame->unsupported|=ZMUX_PROBE_HYPERLINK;
   GhosttyStyle style=GHOSTTY_INIT_SIZED(GhosttyStyle);
   FRAME_CHECK(ghostty_render_state_row_cells_get(p->cells,GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,&style));
   if(style.faint||style.blink||style.invisible||style.strikethrough||style.overline
    ||style.underline_color.tag!=GHOSTTY_STYLE_COLOR_NONE
    ||(style.underline!=GHOSTTY_SGR_UNDERLINE_NONE&&style.underline!=GHOSTTY_SGR_UNDERLINE_SINGLE))
    frame->unsupported|=ZMUX_PROBE_EXTRA_STYLE;
   ZmuxProbeCell cell={0};cell.row=row;cell.column=current_column;
   cell.width=wide==GHOSTTY_CELL_WIDE_WIDE?2:1;
   if(frame_color(style.fg_color,&cell.foreground)||frame_color(style.bg_color,&cell.background))return -1;
   if(content==GHOSTTY_CELL_CONTENT_BG_COLOR_PALETTE) {
    cell.background.kind=1;
    FRAME_CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_COLOR_PALETTE,&cell.background.index));
   } else if(content==GHOSTTY_CELL_CONTENT_BG_COLOR_RGB) {
    GhosttyColorRgb rgb;FRAME_CHECK(ghostty_cell_get(raw,GHOSTTY_CELL_DATA_COLOR_RGB,&rgb));
    cell.background.kind=2;cell.background.r=rgb.r;cell.background.g=rgb.g;cell.background.b=rgb.b;
   }
   cell.bold=style.bold;cell.italic=style.italic;
   cell.underline=style.underline!=GHOSTTY_SGR_UNDERLINE_NONE;cell.inverse=style.inverse;
   uint8_t bytes[256];GhosttyBuffer buffer={.ptr=bytes,.cap=sizeof(bytes)};
   if(wide!=GHOSTTY_CELL_WIDE_SPACER_HEAD)
    FRAME_CHECK(ghostty_render_state_row_cells_get(p->cells,GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,&buffer));
   size_t length=buffer.len?buffer.len:1;
   if(length>text_cap-used)return -2;
   cell.text_offset=(uint32_t)used;cell.text_length=(uint32_t)length;
   if(buffer.len)memcpy(text+used,bytes,length);else text[used]=' ';
   used+=length;out[count++]=cell;
  }
  if(column!=frame->cols)return -1;row++;
 }
 if(row!=frame->rows)return -1;
 frame->cells_length=count;frame->text_length=used;return 0;
 #undef FRAME_CHECK
}
