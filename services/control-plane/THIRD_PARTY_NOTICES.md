# Control-plane dependency notices

The Zmux control-plane source is MIT licensed. Its pinned npm dependency graph is in
`bun.lock`; dependencies retain their own licenses and copyrights. No cmux service code,
assets, deployment identifiers, or infrastructure configuration are included.

All resolved packages were reviewed from installed package metadata on 2026-10-14.
The graph contains MIT, ISC, and Apache-2.0 packages only. The direct runtime packages are:

| Package | Version | License | Source |
| --- | --- | --- | --- |
| `better-auth` | 1.7.8 | MIT | <https://github.com/better-auth/better-auth> |
| `pg` | 8.16.3 | MIT | <https://github.com/brianc/node-postgres> |

Direct development packages are `typescript` 5.9.3 (Apache-2.0), `@types/bun` 1.3.5
(MIT), and `@types/pg` 8.15.6 (MIT). Better Auth's resolved packages, Kysely, Jose,
Noble, Zod, and the PostgreSQL client packages are permissively licensed. Distributions
must retain the license files shipped inside `node_modules`; the production image's
installed packages include those files.

Better Auth copyright and MIT terms are in `node_modules/better-auth/LICENSE.md`.
node-postgres copyright and MIT terms are in `node_modules/pg/LICENSE`. Apache-2.0
terms for TypeScript and OpenTelemetry are available in their upstream packages and
repositories. This notice is an inventory, not a replacement for those license texts.
