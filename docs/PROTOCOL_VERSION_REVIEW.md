# Protocol version review — 2026-10-08

This review supplements the router security regressions. Protocol source/IDL
compatibility, current deployed instruction parsing, and successful execution of
a complete swap are different claims. This review does not assert that every
live pool or Token-2022 extension has been simulated.

## Dependency updates and confirmed fixes

- Pin `sol-trade-sdk` to 5.0.7 (released 2026-10-05), adopting its PumpSwap signed
  reserve, current fee schedule, fee recipient and refreshed RPC snapshot fixes.
- Pin `sol-parser-sdk` to 0.7.10 (released 2026-10-07). It fixes PumpFun create_v2
  matching, preserves authoritative quote fields and historical CreateEvent
  decoding, and includes the preceding PumpSwap/CPMM account decoding updates.
- Encode PumpFun's new `partial_fill: OptionBool(false)` explicitly: native
  exact-SOL-in is 26 bytes; V2 exact-quote-in is 25 bytes. Account layouts and
  sell argument lengths stay unchanged. Partial fills remain disabled because
  the router's exact-input check requires the declared spend.
- AMM v4 V2 quotes now use `swap_fee_numerator / swap_fee_denominator` from
  loaded pool params. The previous implementation used `trade_fee_numerator`
  with a fixed 10,000 denominator, and its adapter hardcoded both numerators.
  Official processor source uses the swap fee fraction and rounds input fees up.
  Exact-output inversion uses that same fraction. Direct pool constructors must
  supply the new `swap_fee_denominator`; ordinary pools usually use 10,000.
- PumpSwap rejects sell quotes whose rounded fees exceed gross output instead
  of silently saturating to zero. Regression tests cover positive/negative
  virtual reserves, exhausted effective reserves, and current calculator parity.
- Newly added trade-sdk `StonkFunQuoteRoute` and `StonkFunSolHop::Route` cannot be
  represented as one `RoutedMarket`/single bridge. Conversion explicitly returns
  an error; use the trade-sdk route executor for those funding graphs.

## Current official SDK and logic comparison

Published official packages inspected on 2026-10-08 (package metadata includes
version, tarball and integrity; extracted sources were used rather than only
README examples):

| Official SDK | Version | Source |
| --- | --- | --- |
| Pump | 3.2.0 | [npm](https://www.npmjs.com/package/@pump-fun/pump-sdk), `src/fees.ts`, `src/bondingCurve.ts`, published IDL |
| PumpSwap | 2.1.0 | [npm](https://www.npmjs.com/package/@pump-fun/pump-swap-sdk), fee math and swap builders |
| Raydium | 0.2.73-alpha | [npm](https://www.npmjs.com/package/@raydium-io/raydium-sdk-v2), LaunchLab/CPMM/AMM/CLMM builders and math |
| Orca Whirlpool | 0.22.0 | [npm](https://www.npmjs.com/package/@orca-so/whirlpools-sdk), swap-v2 and quote dependencies |
| Meteora DLMM | 1.9.14 | [npm](https://www.npmjs.com/package/@meteora-ag/dlmm), swap2 and bin quote logic |
| Meteora DAMM v2 | 1.5.1 | [npm](https://www.npmjs.com/package/@meteora-ag/cp-amm-sdk), published bundle, swap2 and fee scheduler logic |

The Rust sources inspected are trade-sdk 5.0.7 and parser-sdk 0.7.10, including
params, RPC loaders, parser events and instruction math. Their latest main heads
were respectively `63b7370420a2a2018d6b64535daf52d9f839e02f` and
`07c9ff28971e637d43a239d7aff8ce1dd96d153b`; subsequent main changes were documentation,
not a newer runtime release. Versions are pinned for reproducibility; "latest"
is this dated snapshot, not an automatic promise about future upgrades.

Additional confirmed logic fixes:

- PumpFun no longer interprets a zero fee as a static 95/30 bps default. Local
  quotes require known rates, round protocol and creator fees separately and
  apply the `buy_exact_sol_in` IDL's split-ceil correction and net-input-minus-one
  formula. The generic JS SDK exact-token `buy` helper has different rounding
  and is not substituted for this instruction. Cold RPC loading refreshes
  Global, FeeConfig, curve and mint owners in one account batch, selects
  SOL/stable/exotic schedules and configurable creator rates, refreshes
  cashback and allowed recipients, and rejects changed creator snapshots.
- PumpFun params retain fee-sharing creator-vault resolution, normalize native
  SOL sentinels, and reject completed curves. Unknown fee state can only bypass
  local quoting with an explicit final `min_out`; a bridge sell still requires
  a quote in intermediate units. Pump 3.2.0 V3/post-completion/nested quote
  routing is **not implemented** and is rejected rather than mapped to V1/V2.
- PumpSwap params retain current protocol/buyback recipient overrides. Event
  snapshots include cashback in the creator-side fee bucket; params already
  contain the combined bucket and are not charged twice.
- CPMM event `base_input` means exact-input versus exact-output instruction,
  not canonical token0/token1 direction. A standalone swap event no longer
  constructs a fabricated zero-fee canonical pool. LaunchLab trade events and
  CPMM pool skeletons lack config/mint fee state, so automatic quotes fail until
  authoritative state is supplied. Known zero rates remain valid.
- DAMM v2 exact-out now serializes **output amount, maximum input** in that
  order. Its inaccurate reserve-only constant-product quote was removed:
  official math uses sqrt price/liquidity and dynamic/time/market-cap/limiter
  fees. High-level exact-input trades select mode 0; low-level partial-fill mode
  1 is rejected. Params/event adapters include the Instructions sysvar for
  rate-limiter paths; full execution of limiter pools was not simulated.
- DAMM/CLMM/Whirlpool/DLMM external quotes bind both input size and input mint.
  Historical swap output is never reused as the current quote; merge updates
  invalidate cached outputs. Explicit one-hop min_out does not require a stale
  historical size match. Concentrated quote helper functions return the stored
  amount-bound value; callers outside RouterClient must still validate direction.

API migration: PumpFun quote functions now return `Result<u64>`; PumpFun,
LaunchLab and CPMM structs require `fee_rates_known`; externally quoted pool
structs require `quoted_input_mint`. Mark rates known only with actual config
and mint fee state. Feed concentrated venues a fresh official quote and token
program owners, or supply an explicit bound. Some event-only adapters infer SPL
Token when the owner is absent; that does not validate arbitrary Token-2022
mints. PumpFun cold refresh conservatively rejects transfer fees/hooks and
other unsupported mint extensions; transfer hooks for the other builders are
also outside this audit's supported account encoding.

## Follow-up compatibility checks

Registry `latest` tags and both Rust main heads were queried again; versions
above remain current. Package tarball integrity values and additional source
pins are saved in [PROTOCOL_SOURCE_SNAPSHOT.json](PROTOCOL_SOURCE_SNAPSHOT.json).

- PumpSwap manual params without a recipient override now honor Mayhem mode
  instead of selecting the ordinary pool fallback. Current GlobalConfig
  recipient overrides still take precedence; for production cache loading use
  authoritative current recipients, since fallback constants can become stale.
- Whirlpool previously silently discarded all tick arrays after the first
  three. It now accepts three fixed plus up to three supplemental arrays,
  serializes `Some(RemainingAccountsInfo)` with `SupplementalTickArrays` variant
  6 and appends writable accounts in the matching order. More than three
  supplemental arrays fail as in the official SDK/program. Parser events still
  expose only the three fixed tick arrays; supply supplemental arrays via the
  params/cached pool when needed.
- CPMM official JS package 0.2.73-alpha still rounds input-side trade/creator
  fees separately in `curve/calculator.ts`. The current official
  [chain calculator](https://github.com/raydium-io/raydium-cp-swap/blob/b3187ae53a1b95a201f855a59024a12ca8f5b51a/programs/cp-swap/src/curve/calculator.rs)
  combines and rounds the total before splitting fees. Router and Rust
  trade-sdk follow the chain behavior. A differential test covers 144 input
  cases across both directions, all creator-fee modes, creator enabled/disabled,
  capped transfer-fee schedules and varied amounts; it also checks exact-output
  inversion returns the smallest input meeting the requested output.

The attempt to refresh deployment-slot metadata for all programs could not
complete: both publicnode and the Solana public RPC timed out/reset requests.
Existing successful PumpFun/mainnet-router evidence below remains dated to its
recorded snapshot; this follow-up does not claim all current deployed binaries
match the inspected sources. Full successful swaps across every live pool and
extension are still outside the validation performed.

## Official ABI coverage

References are pinned to the upstream commit inspected, not a mutable branch.

| Protocol | Official reference | Result / boundary |
| --- | --- | --- |
| PumpFun | [IDL](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/idl/pump.json) | Checked native buy/sell and V2 discriminators, ordered accounts and arguments. Added explicit partial-fill flag. V2 quote token programs/ATAs and sharing config are present. |
| PumpSwap | [IDL](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/idl/pump_amm.json), [fee-recipient migration](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/docs/BREAKING_FEE_RECIPIENT.md) | Base accounts and buy/sell arguments match; existing builders already append pool-v2, buyback recipient/quote ATA and cashback accounts. Fee schedules and signed offsets must come from current snapshots. |
| Raydium LaunchLab (Bonk/StonkFun) | [builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/launchpad/instrument.ts) | 32-byte exact-in payload and 18-account zero-share-fee layout match, including platform/creator claim vaults. Arbitrary share-fee receiver and graduation partial fills are not enabled by this router. |
| Raydium CPMM | [builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/cpmm/instruction.ts) | Swap input/output layouts retain 13 accounts and 24-byte payloads. The recent creator-fee protocol-share change concerns collection account lists; this router builds swaps, not collections. Updated dependencies supply current config decoding. |
| Raydium AMM v4 | [V2 builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/liquidity/instruction.ts), [processor](https://github.com/raydium-io/raydium-amm/blob/d26944bfb76fb5fa8f91e5d440c2050ed358ef81/program/src/processor.rs) | V2 tags 16/17 and 8-account layout match. Fixed input fee fraction. Official builder uses SPL Token; Token-2022 support must not be inferred from a configurable program pubkey. Swap events alone do not carry authoritative fee fractions; refresh account state before quoting a custom-fee pool. |
| Raydium CLMM | [swap_v2 builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/clmm/instrument.ts) | 41-byte payload, 13 fixed accounts, optional bitmap extension then tick arrays match. Requires current tick state and an amount/direction-matched quote. |
| Orca Whirlpool | [generated swap_v2](https://github.com/orca-so/whirlpools/blob/f2a3d13fa04eb15cf5b5a309ef9b226fd5d34e36/rust-sdk/client/src/generated/instructions/swap_v2.rs) | 15 fixed accounts and 43-byte payload with `remaining_accounts_info=None` match. Up to 3 supplemental tick arrays now append writable remaining accounts and the official enum-6 slice (49-byte payload). Transfer-hook slices are still unsupported. |
| Meteora DLMM | [IDL](https://github.com/MeteoraAg/dlmm-sdk/blob/576919e3e4368e542c402f000b4264724f7f23ec/idls/dlmm.json) | swap2 discriminator, 16 fixed accounts and 28-byte payload with empty remaining-account slices match. Bin arrays follow fixed accounts; transfer-hook slices are not encoded. |
| Meteora DAMM v2 | [swap context](https://github.com/MeteoraAg/damm-v2/blob/6cd2614ff61206b456961d0704446da83be5c77a/programs/cp-amm/src/instructions/swap/ix_swap.rs) | swap2 argument struct is two u64s plus mode; optional referral sentinel/event accounts are retained. Corrected exact-out amount order; adapters append Instructions sysvar for limiter paths. Reserve-only quotes and partial fills are unsupported. |

Token-2022 program selection alone does not imply extension support. Transfer
fees require current epoch/mint schedules; transfer hooks require additional
accounts. Concentrated-liquidity quotes require current ticks/bins and direction,
not merely reserves or an earlier event's output. Dynamic following legs still
accept only their deliberately supported instruction layouts; upgrading a DEX
must not bypass the router's mint or budget guards.

## Mainnet PumpFun account snapshot

Read-only `getMultipleAccounts` at confirmed slot `454461315` returned Global
(owner Pump, 1087 bytes) and FeeConfig (owner pump-fees, 4097 bytes), with the
expected discriminators. Global offsets 105/154 contained protocol/creator
fallback rates 95/5 bps; offset 1045 enabled configurable creator fees. This
validates those live account fields, not a complete cold-loader trade or every
fee tier. No signatures or broadcasts were involved.

## Mainnet PumpFun parsing probe

Fetched the deployed PumpFun program using read-only RPC, then loaded its ELF
into a local LiteSVM. No mainnet transaction was submitted.

- Program: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- ProgramData: `B5MvUwXdiW1NMM6QFFD3ssPKBujD4zMohncbM73Z2BQu`
- Deployment slot observed: `452654932`
- SHA256 of ELF plus allocated padding:
  `1023f7b01210f102713d52beff506ab1cc940246831a5128f004115e49c80f99`

Both old/new native buy lengths (25/26) and V2 buy lengths (24/25) reach account
validation and fail with `AccountNotEnoughKeys` (3005) when given no accounts.
Therefore missing the new flag is **not proven to cause a current mainnet
serialization failure**. Explicit false aligns the latest published ABI and is
accepted by the observed deployment. This probe checks instruction parsing, not
complete swap execution or partial-fill behavior at graduation.

## Validation and deployment

165 SDK tests, 14 program tests and 2 example tests pass with upgraded pins.
SDK tests include the new non-default AMM fee fraction/inverse checks and signed
PumpSwap quote cases; network-gated tests are disabled in this run. Workspace
compilation is checked separately. The chain processor has not changed in this
supplement; its six SBF/LiteSVM security cases were verified in the preceding
security fix. Source changes have not upgraded the mainnet router deployment.


## Review coverage

OCR delegate selected 15 Rust code files in this supplement; all 15 were
manually reviewed (15/15, 100%, no code files skipped). The Markdown report was
excluded by OCR's extension filter and reviewed separately. No OCR LLM review,
GitHub Actions workflow, deployment or mainnet transaction was run.

Follow-up review: all 5 OCR-selected files reviewed (4 Rust files plus the JSON
source manifest; 5/5, 100%, no files skipped). The Markdown report was excluded
by the extension filter and reviewed separately.
