// SPDX-License-Identifier: MIT
// Original fixed metadata sidecar, attached to the existing single Integrated owner.
#ifndef ZMUX_FEASIBILITY_METADATA_H
#define ZMUX_FEASIBILITY_METADATA_H
#include "integrated.h"
enum { ZMUX_METADATA_BYTES = 20480, ZMUX_METADATA_RECORDS = 1024, ZMUX_METADATA_EVENT = 4 };
typedef struct { uint32_t source, subcommand, status, terminator, offset, length; } ZmuxMetadataRecord;
typedef struct Metadata Metadata;
Metadata *metadata_new(size_t, size_t, size_t, size_t);
Integrated *metadata_integrated(Metadata *);
uint32_t metadata_failure(const Metadata *);
int metadata_copy(Metadata *, ZmuxMetadataRecord *, size_t, size_t, size_t *,
                  uint8_t *, size_t, size_t *);
int metadata_commit(Metadata *, uint64_t);
void metadata_free(Metadata *);
#endif
