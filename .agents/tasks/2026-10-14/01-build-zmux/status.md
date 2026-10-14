# Status

- **State:** planned
- **Updated:** 2026-10-14

## Notes
- User requires cmux PR #1 behavior on Zmux `main` and cohesive, moderate-sized commits.
- Reviewed live PR #1 metadata and description; its reported checks are not Zmux validation.
- Created the implementation plan and supporting architecture, roadmap, and licensing notes.
- Planning documents committed on `main`; no application code, license file, build configuration, or remote resources created.
- No remote is configured; the planning commit is local.
- Proposed defaults: macOS first, Swift native app, Rust remote agent, SSH first,
  optional TypeScript cloud services, MIT first-party code with reviewed permissive dependencies.
- These are planning choices, not verified technology selections. Engine integration is an M0 gate.

## Next action
When implementation is requested, begin M0: confirm copyright holder and supported
macOS baseline, establish the license inventory, and evaluate upstream terminal engine
embedding on macOS. Select the engine before committing to terminal adapter APIs.

## Outstanding decisions
- Copyright holder for the MIT notice; public product/name availability.
- Minimum macOS version and availability of a macOS build/dogfood machine.
- Whether every dependency must literally be MIT, or compatible permissive licenses are acceptable.
- Remote host targets: propose Linux first and macOS second.
- Initial release channel: propose signed/notarized direct downloads; defer app stores.
- Cloud provider, operating budget, and whether peer-to-peer connectivity is needed at all.
