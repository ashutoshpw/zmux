// SPDX-License-Identifier: MIT
// Original owner-to-owner C interface. The Resource never exports an engine layout.
#ifndef ZMUX_FEASIBILITY_RESOURCE_INTERNAL_H
#define ZMUX_FEASIBILITY_RESOURCE_INTERNAL_H
#include <ghostty/vt.h>
#include "resource.h"
typedef struct Resource Resource;
typedef GhosttyResult (*ResourceBindFn)(GhosttyTerminal, void *);
typedef GhosttyResult (*ResourceCreateFn)(const GhosttyAllocator *, const GhosttyAllocator *,
  GhosttyTerminal *, uint16_t, uint16_t, size_t, void *);
Resource *resource_new(uint16_t, uint16_t, size_t, size_t, size_t, size_t, size_t, uint8_t);
Resource *resource_new_factory(uint16_t, uint16_t, size_t, size_t, size_t, size_t, size_t,
                              uint8_t, ResourceCreateFn, void *);
int resource_ready(Resource *);
int resource_stats(Resource *, ZmuxResourceStats *, size_t);
void resource_close(Resource *);
void resource_free(Resource *);
int resource_limits(Resource *, size_t, size_t, size_t);
int resource_feed(Resource *, const uint8_t *, size_t);
int resource_resize(Resource *, uint16_t, uint16_t);
int resource_clone(Resource *, size_t *);
int resource_history(Resource *, size_t *);
int resource_scroll(Resource *, int32_t);
int resource_copy(Resource *, uint8_t *, size_t, size_t *);
int resource_snapshot(Resource *, uint8_t *, size_t, size_t *, uint16_t *, uint16_t *);
int resource_bind_callbacks(Resource *, ResourceBindFn, void *);
size_t resource_host_storage_bytes(void);
#endif
