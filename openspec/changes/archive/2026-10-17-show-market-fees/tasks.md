# Tasks

## 1. Ground truth

- [x] 1.1 Download CCP's current static data export, generate `crates/sekondo/data/npc_corporation_factions.csv` (`corporation_id,faction_id`) from `npcCorporations` with a header comment naming the SDE build and URL, and add a script under `scripts/` that regenerates it. Verify: the script reproduces the checked-in file, and 1000035 (Caldari Navy, the owner ESI reports for Jita 4-4) maps to the faction the SDE lists for it.
- [x] 1.2 Manual in-game check with the user (design, Open Questions). Note the market window's broker fee at one NPC station for a character with Connections trained and a known Broker Relations level. Fetch its raw `/characters/{id}/standings` with the dev token and compare against 3 − 0.3·BR − 0.03·F − 0.02·C. Record the result (raw or effective, and whether the coefficients match) in design.md. Verify: design.md's Open Questions entry is answered. If ESI reports effective standings or the numbers don't match, stop and revisit the plan.

## 2. Scopes

- [x] 2.1 Add `esi-skills.read_skills.v1` and `esi-characters.read_standings.v1` to `oauth::REQUIRED_SCOPES`, update the stub's default granted scopes, and extend the login-start, callback and token tests to six scopes, including a character linked with the four old scopes becoming "needs re-login". Verify: `cargo test -p sekondo --test login_start --test callback --test tokens` passes.

## 3. Contracts and stub

- [x] 3.1 Add the contracts `skills/levels`, `skills/forbidden`, `standings/list`, `standings/forbidden` and `stations/station` to `esi-contract`. Verify: the contract unit tests pass.
- [x] 3.2 Add the stub routes `GET /characters/{id}/skills`, `GET /characters/{id}/standings` and `GET /universe/stations/{id}`, scope checks returning 403, and the controls `/_stub/skills`, `/_stub/standings` and `/_stub/stations`. Verify: a new `eve-stub/tests/esi_fees.rs` covers success and 403, and the coverage and conformance tests pass.
- [x] 3.3 Extend `esi-verify` to cover skills, standings (failing with "a standing is needed" when there is no NPC row) and the station of an NPC order (Jita 4-4, 60003760, as fallback). Verify: `crates/esi-verify/tests/verify.rs` covers the missing-standing failure and the fallback station.
- [x] 3.4 Run the real-EVE verification with the user's verification character and commit the new `verified-stub.json`. Verify: stub-backed tests run again.

## 4. ESI client and storage

- [x] 4.1 Add `EsiClient::skills`, `standings` and `station` with gate routes and decoding (standing parsed from its decimal text, not via `f64` arithmetic). Verify: `tests/esi_client.rs` cases against the stub, including a standing with more than four decimals kept exactly.
- [x] 4.2 Write the migration for `character_skills`, `character_standings`, `station_owners`, `fee_sync` and `npc_corporation_factions` (rows generated from the CSV), all per-character tables cascading. Verify: the `tests/schema.rs` and `tests/unlink.rs` cases show the rows go away on unlink and owner change.

## 5. Fee math

- [x] 5.1 Implement `fees::FEES_SQL` (design §5) with a query function for the selected characters of an account. Verify: `tests/fees.rs` covers the spec scenarios:
  - Accounting 5 → 3.375
  - no Accounting → 7.5
  - BR5 with F 2.0 and C 1.5 → 1.41
  - BR0 with F −5.0 → 3.15
  - minimum margin 6.5063…
  - structure → broker fee and margin NULL
  - unknown faction → F 0
  - never-polled skills or standings, or an unknown owner → NULL figures
  - account isolation

## 6. Fee poller

- [x] 6.1 Implement `fee_poller.rs` for skills and standings (hourly, `Expires`, backoff, re-login skip, keep last known on failure, only the two skills and the NPC standings stored). Verify: `tests/fee_poll.rs` covers each `fee-sync` polling scenario against the stub.
- [x] 6.2 Add station owner resolution to the fee poller tick: unknown NPC stations of current orders, one ESI call per station across accounts, retry on failure. Verify: `tests/fee_poll.rs` covers a station shared by two accounts being requested once.
- [x] 6.3 Add `Event::FeesUpdated` (`fees-updated`), published after commit when the `FEES_SQL` result of a character changes through a skills poll, a standings poll or an owner lookup, and not otherwise. Wire the poller into `main.rs` and add a test-hooks route `POST /_test/fee-poll`. Verify: the `tests/events_stream.rs` cases cover the signal on a change, none on an unchanged poll, account isolation, and no payload. `tests/no_test_hooks.rs` covers the new hook.

## 7. Orders page

- [x] 7.1 Extend `FreshnessView` with fee locations and render them in `orders_freshness.html` (two decimals, four-decimal `title` with inputs, "0.5% + owner fee", "—"), ordered by location name. Verify: the `tests/` cases for the orders freshness fragment, plus a Playwright test in `e2e/tests/fees.spec.ts` covering the NPC station figures, the hover text, a station and a structure side by side, a newly linked character showing "—", and a narrowed selection.
- [x] 7.2 Add `sse:fees-updated` to the `#freshness` trigger in `orders.html`. Verify: a Playwright test in `e2e/tests/fees.spec.ts` sets a higher Accounting level in the stub, triggers `/_test/fee-poll`, and sees the new sales tax without a reload while the order tables do not re-fetch.

## 8. Docs

- [x] 8.1 Update README with the two new scopes, the forced re-login, the faction table and how to regenerate it. Verify: README mentions all three.
