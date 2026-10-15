// SPDX-License-Identifier: MIT
// Original 64-bit C consumer of the pinned public and original extension headers.
#include <stddef.h>
#include <stdio.h>
#include <ghostty/vt/zmux_metadata.h>
#include "frame.h"
#include "metadata.h"

_Static_assert(sizeof(void *) == 8, "the reviewed slices require a 64-bit ABI");
_Static_assert(sizeof(ZmuxGhosttyMetadataV1) == 40, "metadata descriptor ABI changed");
_Static_assert(offsetof(ZmuxGhosttyMetadataV1, payload) == 24, "metadata payload ABI changed");
_Static_assert(sizeof(ZmuxProbeFrame) == 72, "frame ABI changed");
_Static_assert(sizeof(ZmuxProbeCell) == 28, "cell ABI changed");
_Static_assert(sizeof(ZmuxMetadataRecord) == 24, "metadata record ABI changed");
_Static_assert(sizeof(ZmuxResourceStats) == 80, "resource ABI changed");
_Static_assert(sizeof(ZmuxIntegratedStats) == 136, "integrated accounting ABI changed");
_Static_assert(sizeof(ZmuxIntegratedFrame) == 40, "integrated frame ABI changed");
_Static_assert(sizeof(ZmuxEffectEvent) == 12, "effect ABI changed");

// A real relocation to the extension constructor must resolve when linking this
// independent C consumer. Volatile prevents eliminating the address check.
static GhosttyResult (*volatile metadata_constructor)(
    const GhosttyAllocator *, const GhosttyAllocator *, GhosttyTerminal *,
    uint16_t, uint16_t, size_t, size_t, void *, ZmuxGhosttyMetadataCallbackV1
) = zmux_ghostty_terminal_new_metadata_bounded_v1;

int main(void) {
    if (!metadata_constructor) return 1;
    puts("darwin_abi_success=true pointer_bits=64 frame=72 cell=28 metadata=40");
    return 0;
}
