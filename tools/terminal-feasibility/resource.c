// SPDX-License-Identifier: MIT
// Original MIT mapping-quota fixture. All engine mappings are dedicated and zeroed.
#define _DEFAULT_SOURCE
#include <ghostty/vt.h>
#include <limits.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include "resource.h"

typedef struct Probe Probe;
extern Probe *probe_wrap_terminal(GhosttyTerminal, const GhosttyAllocator *);
extern void probe_free(Probe *);
extern int probe_feed(Probe *, const uint8_t *, size_t);
extern int probe_resize(Probe *, uint16_t, uint16_t);
extern int probe_copy(Probe *, uint8_t *, size_t, size_t *, uint16_t *, uint16_t *);
extern int probe_scroll(Probe *, int32_t);
extern GhosttyResult zmux_ghostty_terminal_new_bounded(const GhosttyAllocator *,
  const GhosttyAllocator *, GhosttyTerminal *, uint16_t, uint16_t, size_t);
extern GhosttyResult zmux_ghostty_terminal_clone_storage(GhosttyTerminal, size_t *);

enum { MAX_MAPPINGS = 4096 };
typedef struct Resource Resource;
typedef struct { Resource *owner; uint8_t kind; } Context;
typedef struct {
  void *pointer;
  size_t requested, reserved;
  uint8_t alignment, kind;
} Mapping;
struct Resource {
  Probe *probe;
  GhosttyTerminal terminal; // Borrowed from probe after construction.
  GhosttyAllocator general_allocator, page_allocator;
  Context contexts[2];
  Mapping mappings[MAX_MAPPINGS];
  ZmuxResourceStats stats;
  size_t total_limit, general_limit, page_limit, page_size, deny_at;
};

static Mapping *find(Resource *r, const void *pointer) {
  for (size_t i = 0; i < MAX_MAPPINGS; i++)
    if (r->mappings[i].pointer == pointer) return &r->mappings[i];
  return NULL;
}
static void *allocate(void *ctx, size_t length, uint8_t alignment, uintptr_t address) {
  (void)address;
  Context *context = ctx;
  Resource *r = context->owner;
  r->stats.calls++;
  if (context->kind) r->stats.page_calls++; else r->stats.general_calls++;
  if (!length || alignment >= sizeof(size_t) * CHAR_BIT
      || ((size_t)1 << alignment) > r->page_size
      || length > SIZE_MAX - (r->page_size - 1)) {
    r->stats.denials++;
    return NULL;
  }
  size_t bytes = (length + r->page_size - 1) & ~(r->page_size - 1);
  uint64_t used = context->kind ? r->stats.pages : r->stats.general;
  size_t limit = context->kind ? r->page_limit : r->general_limit;
  if ((r->deny_at && r->stats.calls == r->deny_at)
      || bytes > r->total_limit - r->stats.reserved || bytes > limit - used) {
    r->stats.denials++;
    return NULL;
  }
  Mapping *slot = find(r, NULL);
  if (!slot) { r->stats.denials++; return NULL; }
  void *pointer = mmap(NULL, bytes, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANON, -1, 0);
  if (pointer == MAP_FAILED) { r->stats.denials++; return NULL; }
  *slot = (Mapping){pointer, length, bytes, alignment, context->kind};
  r->stats.reserved += bytes;
  if (context->kind) r->stats.pages += bytes; else r->stats.general += bytes;
  if (r->stats.reserved > r->stats.peak) r->stats.peak = r->stats.reserved;
  r->stats.live++;
  return pointer;
}
static int valid(Context *context, Mapping *slot, size_t length, uint8_t alignment) {
  if (!slot || slot->requested != length || slot->alignment != alignment
      || slot->kind != context->kind) {
    context->owner->stats.invalid++;
    return 0;
  }
  return 1;
}
static bool resize_mapping(void *ctx, void *pointer, size_t length, uint8_t alignment,
                           size_t new_length, uintptr_t address) {
  (void)address;
  Context *context = ctx;
  Mapping *slot = find(context->owner, pointer);
  if (!valid(context, slot, length, alignment) || !new_length) return false;
  // Retain and charge the entire mapping on shrink. Growth uses allocate/copy/free,
  // so its temporary old+new peak must fit quota before any allocation occurs.
  if (new_length > length) return false;
  slot->requested = new_length;
  return true;
}
static void *remap_mapping(void *ctx, void *pointer, size_t length, uint8_t alignment,
                           size_t new_length, uintptr_t address) {
  return resize_mapping(ctx, pointer, length, alignment, new_length, address) ? pointer : NULL;
}
static void release(void *ctx, void *pointer, size_t length, uint8_t alignment, uintptr_t address) {
  (void)address;
  Context *context = ctx;
  Resource *r = context->owner;
  Mapping *slot = find(r, pointer);
  if (!valid(context, slot, length, alignment)) return;
  if (munmap(pointer, slot->reserved)) { r->stats.invalid++; return; }
  r->stats.reserved -= slot->reserved;
  if (slot->kind) r->stats.pages -= slot->reserved; else r->stats.general -= slot->reserved;
  r->stats.live--;
  memset(slot, 0, sizeof(*slot));
}
static const GhosttyAllocatorVtable vtable = {
  .alloc = allocate, .resize = resize_mapping, .remap = remap_mapping, .free = release
};

Resource *resource_new(uint16_t cols, uint16_t rows, size_t history,
  size_t total, size_t general, size_t pages, size_t deny_at, uint8_t bounded) {
  if (!cols || cols > 240 || !rows || rows > 120 || history > 1000
      || general > total || pages > total || total > 64 * 1024 * 1024) return NULL;
  Resource *r = calloc(1, sizeof(*r));
  if (!r) return NULL;
  long page_size = sysconf(_SC_PAGESIZE);
  if (page_size <= 0 || ((size_t)page_size & ((size_t)page_size - 1))) { free(r); return NULL; }
  r->page_size = (size_t)page_size;
  r->total_limit = total; r->general_limit = general; r->page_limit = pages; r->deny_at = deny_at;
  for (size_t i = 0; i < 2; i++) r->contexts[i] = (Context){r, (uint8_t)i};
  r->general_allocator = (GhosttyAllocator){.ctx = &r->contexts[0], .vtable = &vtable};
  r->page_allocator = (GhosttyAllocator){.ctx = &r->contexts[1], .vtable = &vtable};
  // Compare the unchanged public constructor with the experimental constructor.
  // This is real engine execution; no allocator or terminal behavior is emulated.
  GhosttyResult result = bounded ?
    zmux_ghostty_terminal_new_bounded(&r->general_allocator, &r->page_allocator,
                                    &r->terminal, cols, rows, history) :
    ghostty_terminal_new(&r->general_allocator, &r->terminal, cols, rows);
  if (result == GHOSTTY_SUCCESS) {
    r->probe = probe_wrap_terminal(r->terminal, &r->general_allocator);
    if (!r->probe) r->terminal = NULL; // wrapper consumed/freed terminal on failure.
  }
  return r; // Caller can inspect denied partial construction and zero live mappings.
}
int resource_ready(Resource *r) { return r && r->probe && !r->stats.denials && !r->stats.invalid; }
int resource_stats(Resource *r, ZmuxResourceStats *out, size_t size) {
  if (!r || !out || size != sizeof(*out)) return -1;
  *out = r->stats; return 0;
}
void resource_close(Resource *r) {
  if (r && r->probe) { probe_free(r->probe); r->probe = NULL; r->terminal = NULL; }
}
void resource_free(Resource *r) {
  if (!r) return;
  resource_close(r);
  // Clean only fixture-owned mappings on a red leak; acceptance checks stats first.
  for (size_t i = 0; i < MAX_MAPPINGS; i++)
    if (r->mappings[i].pointer) munmap(r->mappings[i].pointer, r->mappings[i].reserved);
  free(r);
}
int resource_limits(Resource *r, size_t total, size_t general, size_t pages) {
  if (!r || total > r->total_limit || general > r->general_limit || pages > r->page_limit
      || total < r->stats.reserved || general < r->stats.general || pages < r->stats.pages) return -1;
  r->total_limit = total; r->general_limit = general; r->page_limit = pages; return 0;
}
int resource_feed(Resource *r, const uint8_t *bytes, size_t count) {
  if (!resource_ready(r) || count > 4096) return -1;
  int result = probe_feed(r->probe, bytes, count);
  return resource_ready(r) ? result : -1;
}
int resource_resize(Resource *r, uint16_t cols, uint16_t rows) {
  if (!resource_ready(r)) return -1;
  int result = probe_resize(r->probe, cols, rows);
  return resource_ready(r) ? result : -1;
}
int resource_clone(Resource *r, size_t *history) {
  if (!resource_ready(r)) return -1;
  GhosttyResult result = zmux_ghostty_terminal_clone_storage(r->terminal, history);
  return resource_ready(r) && result == GHOSTTY_SUCCESS ? 0 : -1;
}
int resource_history(Resource *r, size_t *history) {
  if (!resource_ready(r)) return -1;
  return ghostty_terminal_get(r->terminal, GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS, history) == GHOSTTY_SUCCESS ? 0 : -1;
}
int resource_scroll(Resource *r, int32_t lines) {
  if (!resource_ready(r)) return -1;
  int result = probe_scroll(r->probe, lines);
  return resource_ready(r) ? result : -1;
}
int resource_copy(Resource *r, uint8_t *out, size_t cap, size_t *length) {
  if (!resource_ready(r)) return -1;
  uint16_t x, y;
  int result = probe_copy(r->probe, out, cap, length, &x, &y);
  return resource_ready(r) ? result : -1;
}
size_t resource_host_storage_bytes(void) { return sizeof(Resource) + 8192; }
