# Isolated metadata contract v1

Original MIT experiment against Ghostty
`22fa134a07f5749edd9e92c6bc17d8aaf61843be` plus the separately hashed resource
patch. This contract is an experiment input. Evidence and the production closure
path live in the [metadata reference](../../.agents/tasks/2026-10-14/05-command-ux-parity/references/f04-metadata-contract.md).
No application engine/dependency changes, PTY delivery or cmux parity are claimed.

- The existing Resource alone constructs a terminal/Probe with its general/page
  allocators. Integrated owns the callback/failure/generation protocol. A fixed
  metadata sidecar adds capture storage and its charge exactly once; no second
  parser/terminal or duplicated publication owner exists.
- The opt-in versioned bounded constructor requires a callback and a control cap
  4..16,384 before allocating. Default and original resource constructors do not
  enable metadata. Their parser behavior is unchanged.
- Selected OSC 0/2/7/9/777 captures use the same general allocator without fallback
  to a smaller buffer. The cap counts number digits, first delimiter and payload;
  ESC ], BEL/ESC termination and a reserved semantic-parser sentinel are excluded.
  C0 framing/ESC termination follows the pinned VT parser. In particular, it ends
  OSC at ESC before the following backslash; this is not proof of full-ST validation.
- Command -> Action -> handler -> versioned C callback retains source, subcommand,
  termination byte, status and complete payload. Status 0=complete, 1=CAN/SUB abort,
  2=malformed, 3=policy overflow, 4=allocation failure. Known selected numbers with
  a missing delimiter report malformed at termination. Overflow/allocation status
  is emitted after the failing scalar/bulk parse step without waiting for a
  terminator; a prefix is never reported complete afterward.
- OSC 9 semantic classification reuses the pinned permissive parser before its
  notification fallback: subcommands 1..12 represent its ConEmu meanings; 0 is a
  notification. Their requested effects never run in host-managed mode. OSC 1337
  CurrentDir retains explicit source through its command/action and is ignored by host policy;
  arbitrary OSC 1337 commands retain their original parsing/limits, not the extended
  selected capture contract.
- Title consumes the full post-first-delimiter payload, including semicolons.
  Notification OSC 9 consumes its full fallback body; OSC 777 splits notify/title
  and retains the body remainder. These fix baseline exact-field parser defects.
  CAN/SUB updates are suppressed. Lossy UTF-8, control/bidi filtering precedes
  256-title/128-notice-title/2,048-notice-body Unicode-scalar caps.
- Cwd accepts the full OSC 7 file:// URI, including semicolon path characters
  (an approved exact-field parser repair): decode percent triplets once, keep
  malformed triplets literal, require absolute UTF-8 decoded path <=4,096 bytes
  without controls. Unsupported/empty/missing-prefix/slash/invalid raw UTF-8 keeps
  old cwd; invalid decoded content clears it. It is untrusted metadata, never code,
  navigation authorization or an implicit process cwd mutation.
- BEL/OSC sources remain exact. Empty normalized notice titles are suppressed;
  retain 32 notices with counted oldest eviction and checked sequence exhaustion.
  Host title/cwd/notices survive RIS/DECSTR as approved migration compatibility,
  not established cmux behavior. RESET does not clear accounting/failure/sequence.
- One ordered stream includes BEL, reply, RESET and metadata records. Metadata
  callbacks copy into 1,024 fixed descriptors and 20,480 raw bytes; any overflow
  fails visibly. The combined callback event cap remains 1,024. No callback allocates,
  reenters or invokes UI/PTY/Rust. All source bytes are copied before borrowed
  upstream storage changes.
- Two normalized Rust slots reserve payload storage before engine creation: title
  1,024 bytes, cwd 4,096 and 32*(512+8,192) notice bytes each. Raw scratch, event
  records, inline owner and existing frame/reply capacities are charged separately.
  Reservation is fallible; no normalization/commit/publication allocation occurs.
- Capture/collector/engine/render/normalization failure discards the staged result,
  preserving last good metadata/frame/notices/replies. Current upstream input may
  finish; no replay, interrupt or consumed-byte receipt is promised. Prior successful
  chunks remain published. Successful export seals engine/config access until exact
  nonallocating commit; metadata retirement and slot swap use that same commit.
- Engine getters stay unused/empty for selected host metadata. TITLE_REPORT stays
  disabled and enabling it rejects explicitly; support would need an ordered host query
  contract. Total RSS,
  physical footprint, kernel/allocator overhead and caller-retained copies are not
  covered by reserved-mapping/fixed-storage accounting or experimental budgets.
