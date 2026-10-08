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
| Pump | 4.0.0 | [npm](https://www.npmjs.com/package/@pump-fun/pump-sdk), `src/fees.ts`, `src/bondingCurve.ts`, published IDL |
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
  a quote in intermediate units. Pump V3 now has a separate explicit API,
  described below; the existing TradingClient does not automatically switch
  versions or recursively fund nested quote tokens.
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
program owners, or supply an explicit bound. CLMM and Whirlpool event/account adapters now preserve unknown mint owners
instead of treating them as SPL Token; resolve owners from an authoritative
cache/RPC before building. Some other legacy params/event conversions still
default missing programs to SPL; callers must supply actual owners and this
does not validate arbitrary Token-2022 extensions. PumpFun cold refresh conservatively rejects transfer fees/hooks and
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

## Token-program owner validation supplement

CLMM logs omit mint owner programs. A Token-2022 mint may charge zero transfer
fees, so zero observed fees do not imply classic SPL Token. CLMM snapshots now
leave unknown owners as the zero pubkey; only WSOL/USDC/USDT are inferred as
known classic mints, and positive transfer fees indicate Token-2022. The
`clmm_apply_token_programs` overlay preserves missing values rather than
silently substituting classic SPL. Whirlpool account snapshots follow the same
known-mint rule; filled swap events retain their actual program fields. Merge
updates independently preserve the known owner on the other side.

CLMM, Whirlpool, DLMM, DAMM v2 and PumpSwap leg builders reject missing or
unsupported token programs before deriving ATAs. Supplying a Token-2022 program
still does not implement transfer-hook remaining accounts. AMM v4 V2 now rejects
Token-2022 for both exact-in and exact-out, matching the official processor's
`spl_token::id()` check. Tests verify that a zero-transfer-fee unknown CLMM mint
fails until overlaid with its Token-2022 owner, then uses the correct ATA; owner
merge tests preserve the independently known side; AMM tests cover both modes.

This supplement's 3 changed Rust files were manually reviewed (3/3, 100%, no
code skipped); this document was reviewed separately.

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

168 SDK tests, 14 program tests and 2 example tests pass with upgraded pins.
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


## Explicit Pump V3 API and deployed-program verification

`load_pumpfun_v3_by_rpc(rpc, mint, user)` loads `PumpFunV3Pool` from a
coherent curve/Global/FeeConfig/mint/base-vault/user-volume batch after discovering
quote mint and base mint owner. FeeConfig is mandatory. Both mint owners and
extensions, vault mint/authority/state, and existing volume account ownership,
discriminator and user are checked. Completed curves and cashback coins fail.
Old curve extension and missing volume initialization are returned as setup
instructions before the router's budget measurement.

The API supports `buy_quote`, `buy_quote_for_tokens`, `build_buy_route`,
`build_buy_exact_out_route`, and `build_sell_route`. The 17-account V3 ABI is
independent of V1/V2: it omits the creator vault and ordinary fee recipient,
and uses the curve/user base and quote ATAs, user volume, FeeConfig and buyback
recipient. Token-quote recipients use canonical quote ATAs; native output binds
the router System sentinel. Partial fill is always false. For token-quoted or
nested curves these helpers settle in the quote token, which the caller must
fund; recursive SOL acquisition is not implemented.

V3 exact-quote-input buys can leave rounding dust. Their router route uses the
existing maximum-input bit (historically named exact-out): total input debit is
bounded by the declared budget, the router fee is charged on that full budget,
and both DEX and router enforce the positive output minimum. V3 exact-output
buys use the same maximum-input protection and serialize tokens then maximum
quote cost. Sell routes retain exact debit equality. Setup rent is outside the
trade input budget; `fee_bps` must match the deployed router config.

Official 3.2.0 quotes include a pool-to-be leg when buying beyond the remaining
curve supply. That math is implemented and unit-tested.
`supports_graduation` defaults to **true** in the cold loader, matching the
published official SDK's quote and construction behavior. Callers can explicitly
set it to false to restrict buys to the remaining curve supply. This is an SDK
construction option, not a declaration that a specific deployment accepts it.
The historical 2026-10-08 captured mainnet Pump ELF returns 6021 (`NotEnoughTokensToBuy`) for our
non-Mayhem crossing fixture, including with realistic curve reserve invariants.
This local observation does not prove every possible live crossing fails, and
SDK publication alone does not establish deployed behavior.

Local integration executes **both actual Pump ELF and repaired router ELF**
with public mainnet Global/FeeConfig state captured at slot 454461315 and
synthetic classic-token curves/accounts. It verifies V3 buy, exact-output buy,
base-token router fees, native SOL sell, and graduation rejection with atomic
rollback. The FeeConfig SOL tier is 95/30 bps; Global's 95/5 fallback is not used
for V3. Pump ProgramData deployment slot: 452654932; captured ELF plus padding
SHA256: `1023f7b01210f102713d52beff506ab1cc940246831a5128f004115e49c80f99`.
Token-quoted/nested V3 account layouts and quotes have offline tests; complete
execution on those paths and arbitrary Token-2022 extensions remain unverified.

```sh
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/pump.so
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/pump.so --exact-out
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/pump.so --graduation-rejection
```

Use solders 0.29.0. These scripts never call RPC or submit network transactions.
The public fee fixture is `scripts/fixtures/pump-v3-mainnet-fees.json`. To verify
a new deployment, supply its freshly downloaded ELF and re-evaluate graduation
capability rather than treating a failed rejection assertion as an SDK regression.

Whirlpool supplemental tick arrays are now accepted by both static and dynamic
routes: 49-byte remaining slice encoding with official enum 6 and 1–3 writable
arrays. Dynamic amount patching preserves that slice; malformed options, vector
lengths, enum values, counts, truncated data and missing SDK accounts are tested.
Transfer-hook slices remain outside the supported dynamic whitelist.


### Graduation compatibility recheck — 2026-10-08

The cold-loader default is `supports_graduation=true`, so the SDK permits the
official post-completion quote and construction paths. Explicitly setting false
rejects output **greater than** the remaining curve supply while permitting
buying exactly the remaining supply and completing the curve. The deployment
observations below are retained as execution evidence, not as a default SDK gate.

The official 3.2.0 npm README distinguishes basic V3 (programs-monorepo PR #60)
from post-completion buys/nested curves (PR #61). These are references in the
published README, not independently verified merge/deployment statuses for the
private program repository. Package publication is not a mainnet upgrade signal.

Fresh read-only mainnet queries confirmed:

- At context slot 454473258, Pump's executable program account still points to
  ProgramData `B5MvUwXdiW1NMM6QFFD3ssPKBujD4zMohncbM73Z2BQu`.
- At context slot 454472205, that ProgramData header reports deployment slot
  **452654932**, matching the downloaded ELF used for the tests.
- The saved ELF recognizes V3 trade instructions but returns Anchor **101**
  (`InstructionFallbackNotFound`) for `set_max_curve_depth`, which the same
  official SDK extension lists. This parser probe does not depend on curve state.

The expanded 32-case LiteSVM matrix uses non-Mayhem curves, classic SPL or
Token-2022 base mints (6 decimals), WSOL quote (9 decimals), actual fee state,
and reserve/vault/lamport invariants for a curve near completion. Tests run with
`initialVirtualQuoteReserves=0` and `30_000_000_000`; both behave identically.
The remaining base supply is 1,000,000,000 raw units, and its official
fee-inclusive curve cost is 416,016 lamports (protocol 95 / creator 30 bps).

| Requested trade | Captured deployment result |
| --- | --- |
| Exact-output: remaining minus 1 raw unit | Success, curve remains open |
| Exact-output: exactly remaining | Success, curve becomes complete |
| Exact-output: remaining plus 1 raw unit | 6021, all trade state and router fee roll back |
| Quote-input: 416,015 or 416,016 lamports | Success, output remains below remaining due to rounding |
| Quote-input: 416,017 / 416,116 / 9,900,000 lamports | 6021, all trade state and router fee roll back |

The captured crossing rejection persists across ATA owner choices and zero or
populated initial quote reserves, while ordinary V3 and exact graduation succeed.
These are synthetic states in the actual ELF, not a live pool simulation, and
are not sufficient to conclude that every real mainnet pool rejects crossing.
SDK quote/construction follows the official behavior by default; a transaction
simulation against the target deployment determines execution compatibility.
Offline SDK regressions cover enabled crossing quotes and route construction,
explicit opt-out at remaining plus one, exact graduation, and large maximum
budgets that must not be mistaken for the actual exact-output spend.

```sh
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/pump.so --boundary-matrix
```

Raw deployment headers, ELF hashes and all matrix outcomes are saved in
[PUMP_V3_DEPLOYMENT_RECHECK.json](PUMP_V3_DEPLOYMENT_RECHECK.json).


### Current Pump deployment recheck — 2026-10-09

The official `@pump-fun/pump-sdk` latest package is **4.0.0**, published
2026-10-08T12:53:48.925Z. Compared with 3.2.0, the quote-coin creation seed
calculation changed; the V3 trading math and published IDL remain unchanged.
The router does not construct quote-coin creation instructions. The official V3
quote functions explicitly reject already-complete curves. Crossing inside the
completing buy is supported; trading an already-complete curve before migration
is not. The SDK's completed-curve guards remain correct.

New read-only captures identify Pump deployment slot **454596459**, ELF SHA-256
`a4b32d322295a15666b1293e0028b9b75d688bfce10b574e1094f249f2ce9f16`, and Fee Program
deployment slot **454596501**, ELF SHA-256
`73c679c8dae8d24153fdd0455b557b73662e93ab2ec88831a9e4be2b08a897a4`.
Global/FeeConfig were captured together at slot **454622338**. Global now has
1088 bytes (including `max_curve_depth=1`); the old 1087-byte Global fixture
fails account deserialization against the new Pump program. Both old and current
snapshots are retained. The Fee Program ELF was captured but is **not executed**
by these V3 cases, which read FeeConfig directly.

The existing local runner now accepts `--fee-fixture` and an explicit
`--graduation-supported` expectation. Its admin dispatch probe checks 3005
(missing accounts) on the new program, rather than old error 101; actual trading
cases independently establish crossing support. Fee rates come from the selected
snapshot, and crossing output includes the official pool-to-be leg, independent
ceil fees and migration fee. Exact-output cases also assert actual debit equals
the official fee-inclusive cost plus router fee. Completion means output >= remaining supply.

With the current Pump ELF and repaired local router ELF, all **32 current
boundary cases pass**, including exact-output remaining+1 and quote-input
416,017 / 416,116 / 9,900,000 lamports. Classic SPL and Token-2022 synthetic base
mints and both initial quote reserve variants pass. A second buy on each
completed curve fails with 6005; token/curve state and router fee roll back
(the transaction fee remains charged). Ordinary quote-input and exact-output
buy/sell roundtrips also pass. The **32 historical cases still pass** with the
old ELF/fixture and old rejection expectations. Thus the historical deployment
limitation above does not apply to the current captured Pump program.

```sh
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/current-pump.so \
  --fee-fixture scripts/fixtures/pump-v4-mainnet-fees.json \
  --boundary-matrix --graduation-supported
```

Evidence: `tools/validation/simulation-coverage-20261009/pump-current-deployment-followup/`.
These are locally signed LiteSVM transactions using real captured program ELF
and synthetic curves, not bank simulations of live pools. The mainnet router
program was not deployed or upgraded. Graduation remains enabled by default.

Workspace check: 93 SDK, 15 program and 2 example tests pass; 81 network cases
and one doctest remain explicitly ignored. Review coverage for this follow-up:
10 changed/added files manually reviewed, 0 skipped (100%), including the
snapshot, deployment metadata and logs excluded by OCR's default filters.


### Native Pump buy setup rent follow-up — 2026-10-09

Official Pump 4.0.0 IDL `buy_exact_sol_in` explicitly requires rent in addition
to `spendable_sol_in` for the creator vault and user volume accumulator. Actual
current Pump + Fee Program ELF execute the buy successfully on synthetic state,
but the router rejects with 16 when missing-account rent increases wallet debit
above the declared budget. Preinitializing volume alone is insufficient when the
creator vault is below its rent floor. V3 quote-input buys also reproduce error
16 for fresh users without volume initialization. These are actual local CPI
executions, not inferred from SDK comments.

Native V1 high-level buys now add Pump's idempotent volume initialization and a
router preparation instruction before the route. New **tag 7** takes signer/
writable payer, readonly Pump curve, writable creator vault and System Program,
with no payload. It validates curve owner/discriminator/minimum creator field,
derives the vault PDA from that curve's creator, validates the vault's System
owner and zero data, and transfers only its missing rent floor using current
Rent sysvar. Repeated preparation transfers nothing extra; it cannot redirect
funds to a supplied arbitrary recipient. Native V3 buys always initialize volume
idempotently, even when a cached `needs_volume_initialization` hint says false.
Token-quoted/sell hint behavior stays as before. Setup and swap remain in one
atomic transaction; setup rent is outside trade input and route fee calculation.
No route debit tolerance or budget/fee/mint protection was relaxed.

The updated existing runner passes **40 native-rent cases**: V1 classic SPL and
Token-2022 with initialized/uninitialized volume and zero, partial, exact or
excess creator-vault rent; failing unprepared controls; repeated preparation;
prepared over-budget rejection with complete setup/fee/token rollback; and V3
quote-input/exact-output cases with fresh/existing volume. It also rejects helper
recipient/program substitutions, malformed data and wrong-owner, wrong-
discriminator or truncated curves. Current 32-case graduation matrix and the
full existing router ELF security suite pass. Offline workspace checks pass
93 SDK / 15 program / 2 example tests (81 live tests and one doctest ignored).

```sh
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/current-pump.so \
  --fee-fixture scripts/fixtures/pump-v4-mainnet-fees.json \
  --fee-program /path/to/current-fee-program.so --native-rent-matrix
```

Evidence: `tools/validation/simulation-coverage-20261009/pump-native-rent-followup/`.
Router ELF SHA-256:
`9fce64e47706c4bb2dbacee46be95b9dc90d1ff0540c9637b3dcca9183c95924`.
This follow-up executes the captured Fee Program ELF as well as Pump. Trades use
synthetic local state and ephemeral signatures; no broadcast or deployment.
**The current mainnet router does not implement tag 7**. New V1 high-level setup
requires the repaired router source to be deployed together with the SDK; these
changes do not repair or establish compatibility with the unchanged deployment.
Low-level raw leg callers must supply the necessary preparation themselves.

Review coverage: all 14 changed/added files reviewed (8 code files and 6
documentation/evidence files), zero skipped. No blocking findings remain in
this follow-up. Historical evidence retains its recorded revisions.


### Native Pump first-sell rent follow-up — 2026-10-09

The same setup gap also affected native SOL **output**. Executing current
Pump/Fee/router ELF with a funded curve and preexisting seller token balance
reproduces router error 10 despite the DEX's sell succeeding: creator-vault rent
(V1) or fresh user-volume rent (V3) is charged during the swap, so wallet net
credit falls below the automatic quote's 1% slippage floor. The sell fixture
preserves the original virtual-reserve constant product, real/virtual quote
relationship and vault/supply bounds; it does not require a preceding buy by the
seller. These are synthetic local states, not a claim about every live curve.

The high-level native V1 sell now uses the same idempotent creator-vault top-up
as buys before output measurement. Ordinary sells do not create an unnecessary
user volume account; cashback sells initialize it. Native V3 sells also always
initialize volume even if the cached hint is false. Buy/sell preparation shares
one helper; router output minimum and fee/input debit checks remain strict.
This SDK follow-up uses the existing tag 7; no on-chain source changed here.

The existing native-rent matrix expands from 40 to **86 cases**, adding 32 V1
first-sell cases with four vault funding states, eight V3 first-sell cases,
four cashback sells and two V1 buy/sell roundtrips across classic SPL and
Token-2022 base programs. Unprepared failing controls require exact router error
10 and verify token, vault, curve, fee and volume rollback (transaction fee
retained). Prepared sells require actual quote receipt, exact token debit and
router fee. Cashback cases additionally verify the creator fee accrues in volume
rather than counting that accrued balance as setup rent. Setup rent is measured
from the Rent sysvar floor. Both V1 and V3 roundtrips now check the exact native
sell proceeds, replacing the earlier merely-positive assertion; V1 roundtrips
use the proper sell account layout/discriminator.

The existing Rust construction tests now verify and sign four ordinary/cashback
V1 sells and three V3 buy/exact-output-buy/sell transactions including setup.
All 86 local rent cases, current 32-case graduation matrix, V3 ordinary and
exact-output roundtrips and 110 offline checks pass (93 SDK / 15 program /
2 examples; 81 live cases and one doctest ignored). Evidence:
`tools/validation/simulation-coverage-20261009/pump-native-sell-rent-followup/`.
The router ELF remains
`9fce64e47706c4bb2dbacee46be95b9dc90d1ff0540c9637b3dcca9183c95924`;
prior security evidence for that binary remains valid and was not relabeled as a
new run. The unchanged mainnet router lacks tag 7, so new V1 setup still requires
its source upgrade together with the SDK. No deploy or broadcast occurred.

Review coverage: 10 changed/added files reviewed, zero skipped (100%), including
all documentation and logs excluded by OCR's default filters.


### Disabled Token-2022 Hook compatibility — 2026-10-09

Pump's cold mint validator previously rejected every TransferHook extension,
including an inactive or disabled Hook with no callback program. The existing
negative-state test reproduces this rejection with real bank-created mint bytes.
A TransferHook stores optional authority and program pubkeys (32 bytes each);
zero program bytes mean no callback and do not require extra transfer accounts.
The validator now accepts only that disabled state, requires exact 64-byte Hook
payload and valid TLV bounds, and continues rejecting active/malformed hooks,
transfer fees and other unsupported extensions. No active callback support is
claimed. Both V1 refresh and V3 cold discovery use the same mint validator.

The existing unit test covers three bank mint records (inactive, disabled after
activation, active), each on base/quote sides, plus invalid Hook lengths 0/1/63/65
and truncated bytes. The snapshot records original bank validation slots and
source SHA-256; it is retained in `scripts/fixtures/token-hook-mints-20261008.json`.
The existing Pump ELF runner adds six disabled-Hook buy/sell roundtrips across V1,
V3 quote-input and V3 exact-output. It retains those mint extension bytes and uses
valid TransferHookAccount token-account state; synthetic base supply/authority
are adjusted to match the runner's synthetic curve. These executions use the
current captured Pump and Fee Program ELF and the unchanged repaired router ELF;
outputs and spends match the independent quote. This is local signature/execution
coverage, not a fresh mainnet bank run or arbitrary extension support.

All six Hook roundtrips, the 86-case native-rent regression matrix and 110 offline
checks pass (93 SDK / 15 program / 2 example tests; 81 live cases and one doctest
ignored). A requested existing Pump mainnet roundtrip could not execute because
RPC transport was unavailable and failed explicitly; its redacted log is saved,
not counted as passing. The public default Git heads of trade/parser were also
rechecked as October 7 ancestors of this router's October 8 pinned repair commits,
so replacing the pins with those heads would remove fixes. Dependencies remain
at their tested pins; sibling worktrees were not modified.

```sh
python scripts/test-pump-v3-local.py /path/to/router.so /path/to/current-pump.so \
  --fee-fixture scripts/fixtures/pump-v4-mainnet-fees.json \
  --fee-program /path/to/current-fee-program.so --hook-matrix
```

Evidence: `tools/validation/simulation-coverage-20261009/pump-disabled-hook-followup/`.
No on-chain source change, deployment or broadcast. Existing V1 preparation
still requires tag 7, unavailable in the unchanged mainnet router.

Review coverage: 10 changed/added files reviewed, zero skipped (100%), including
all documentation, fixture bytes and logs excluded by OCR default filters.


### Non-Pump cold mint/Hook guard — 2026-10-09

The pinned trade-sdk CPMM RPC loader decodes Token-2022 transfer fees but does
not reject active TransferHook callbacks. A mock RPC using bank-created mint
bytes and synthetic CPMM pool/config/vault data reproduces the exported router
cold loader returning an active-Hook route before this fix. Router legs do not
resolve Hook callback accounts. The official Whirlpool SDK has a dedicated
extra-account resolver; accepting the mint alone does not implement that support.

After parameter conversion, cold loading now batches fresh mint reads for the
target and any returned CPMM/AMM V4 bridge, checks owner/program agreement and
initialized mint/extension decoding, and rejects active or malformed Hooks.
Inactive/disabled Hooks remain allowed. Shared mint keys are deduplicated;
conflicting expected programs, missing accounts and incomplete responses fail.
Pump target refresh retains its stricter existing validation; its returned
bridge is also checked. This adds a cold RPC read, not hot-path RPC. Cached or
manually supplied market snapshots remain caller-managed. The new snapshot is
not atomic with preceding pool/fee reads, and mint authorities can subsequently
change extensions; this is not a guarantee of future execution or arbitrary
Token-2022 support. Existing fee decoding/quotes and on-chain source are unchanged.

Expanded the existing routed-market helper test rather than adding a new suite:
6 cases invoke the real CPMM cold loader (three Hook records, both mint sides);
252 shared-guard cases cover LaunchLab, CPMM, PumpSwap, DAMM V2, CLMM, Whirlpool
and DLMM, both sides and valid/missing/wrong-owner/uninitialized/malformed-length/
truncated state. Five bridge cases cover inactive/disabled/active CPMM Hooks,
classic AMM V4 and conflicting shared programs. These are mocked RPC account
checks, not executions of all DEX programs or fresh live pool bank simulations.
All 110 offline tests pass (93 SDK, 15 program, 2 example); the existing signing/
serialization regressions run unchanged. 81 live cases and one doctest remain
ignored; no new live execution is claimed in this follow-up.

Fresh npm registry/tarball checks found Whirlpool SDK 0.22.0 and Raydium SDK
0.2.74-alpha, with SHA-512 package integrity verified. The latest Raydium CPMM
instruction source is byte-identical to the previous 0.2.73-alpha capture.
Whirlpool's published Hook resolver and the existing pinned Rust fee decoder
were inspected. Official package URLs, integrity and source hashes are recorded
in the evidence snapshot. The direct SPL interface dependency uses existing
resolved version 3.1.2; trade/parser pins and sibling repositories are unchanged.
This inspection covers Hook handling/CPMM instruction compatibility, not a new
claim that every protocol release or deployment has been exhaustively verified.

Evidence: `tools/validation/simulation-coverage-20261009/cold-mint-hook-followup/`.
No deployment, broadcast or new program binary. Prior mainnet router/header/tag 7
compatibility limitations still apply.

Review coverage: 9 changed/added files reviewed, zero skipped (100%), including
the manifest, official-source snapshot and all documentation/evidence logs.
