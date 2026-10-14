# Licensing and implementation provenance

## Intended policy
All original Zmux implementation, including optional server code, will be MIT licensed.
The working assumption is that reviewed compatible permissive dependencies are acceptable;
this is distinct from requiring every transitive dependency to use MIT literally.
Resolve that distinction before selecting an engine or other substantial dependencies.

Do not claim third-party code becomes MIT because it is bundled with Zmux. Keep upstream
licenses, attribution, patent terms where applicable, and required notices. Audit pinned
versions, transitive dependencies, native libraries, generated code, fonts, icons and
release tooling separately. An upstream project name or top-level license alone is not
proof that every bundled component is acceptable.

## Independent implementation
- Create a fresh implementation and history; do not fork/import cmux source or its submodules.
- Derive requirements from user needs, public platform documentation, and observable behavior.
- General architectural ideas may inform design; do not copy or closely translate code,
  comments, tests, fixtures, UI assets, or authored documentation from cmux.
- Do not ask an agent to translate/rewrite a cmux file into an MIT equivalent.
- Prefer upstream permissive libraries directly, with independently written integration.
- Write original tests from Zmux requirements rather than reproducing cmux tests.
- Record substantive references, their licenses, and what was learned or adopted.
- If questionable material enters a change, stop and assess provenance before merging;
  renaming or moving it to another package does not remove its license obligations.

This conversation has already examined cmux license text and discussed its architecture.
Do not describe this effort as a strict clean-room implementation. Stronger clean-room
assurance requires a separate specification/implementation process and appropriate review;
an agent with inherited source context is not an independent clean-room implementer.

## Inventory to create during M0
For each dependency or imported asset, record: upstream URL, pinned version/commit,
license identifiers and texts, modifications, transitive/bundled obligations, distribution
location, and reviewer/date. Separately record implementation references and provenance.
Automate inventory checks where possible, but manually inspect native bundles and assets.

## Contribution and release rules
Require contributors to submit original or properly licensed work under MIT and disclose
external sources. No proprietary relicensing CLA is needed for the proposed MIT model;
choose a simple provenance attestation policy during M0. Use original Zmux branding.
Name availability and trademark rights are separate from source licensing.

Before each release, verify the first-party license, dependency notices, source/asset
provenance, and actual contents of the distributable. Record unresolved licensing questions
as release blockers rather than making unsupported claims of MIT-only distribution.
