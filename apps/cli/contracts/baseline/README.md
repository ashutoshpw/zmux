# Frozen compatibility baseline

These original audit records are runtime build inputs owned by the CLI. They freeze
functional command identifiers and review routing against cmux commit
`3ba653b9e5afb11d0bc578031142950d6136697e`; they contain no upstream implementation.

The historical copies under task `2026-10-14/05-command-ux-parity/references` remain
archival evidence. CLI compilation and `zmux commands --check` use only this directory,
so archiving task documentation cannot break a release. The existing `apps/cli/**`
Foundation workflow filter covers changes to these inputs on pushes and pull requests.

Initial byte-identical migration SHA-256 values:

- `command-inventory.json`: `f43fa020589e331f7d67a8094028bd946793cf83abfc5df611163eee62db759d`
- `audit-reconciliation.json`: `464aca6474c60e0ec54c27fe1746f2f7e3d8f1f730e3253db65b9a4b992321c3`

Do not revise historical statuses to describe implementation progress. Track current
counterparts in `src/registry/frozen-coverage.json` and executable contracts. A new
baseline needs an explicit source revision and reviewed reconciliation; passing the
catalog check establishes mapping integrity, not behavioral parity.
