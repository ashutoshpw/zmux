// SPDX-License-Identifier: MIT
// Original MIT feasibility fixture. Public upstream API only.
#include <ghostty/vt.h>
#include <stdlib.h>
#include <string.h>
typedef struct {
 GhosttyTerminal terminal;
 GhosttyRenderState render;
 GhosttyRenderStateRowIterator rows;
 GhosttyRenderStateRowCells cells;
 unsigned char replies[4096]; size_t reply_len; int failed;
} Probe;
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
Probe *probe_new(uint16_t cols,uint16_t rows) {
 if(!cols||!rows||cols>240||rows>120)return NULL;
 Probe *p=calloc(1,sizeof(*p));if(!p)return NULL;
 #define CHECK(call) if((call)!=GHOSTTY_SUCCESS){probe_free(p);return NULL;}
 CHECK(ghostty_terminal_new(NULL,&p->terminal,cols,rows));
 CHECK(ghostty_render_state_new(NULL,&p->render));
 CHECK(ghostty_render_state_row_iterator_new(NULL,&p->rows));
 CHECK(ghostty_render_state_row_cells_new(NULL,&p->cells));
 size_t history_bytes=4*1024*1024,history_lines=1000;
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,&history_bytes));
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,&history_lines));
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_USERDATA,p));
 CHECK(ghostty_terminal_set(p->terminal,GHOSTTY_TERMINAL_OPT_WRITE_PTY,reply));
 #undef CHECK
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
