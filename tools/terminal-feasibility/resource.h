// SPDX-License-Identifier: MIT
// Original MIT experiment ABI, not an upstream or application API.
#ifndef ZMUX_RESOURCE_EXPERIMENT_H
#define ZMUX_RESOURCE_EXPERIMENT_H
#include <stddef.h>
#include <stdint.h>
typedef struct {
  uint64_t reserved, peak, general, pages, denials, invalid;
  uint64_t calls, general_calls, page_calls, live;
} ZmuxResourceStats;
#endif
