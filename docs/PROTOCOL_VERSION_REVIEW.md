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

## Official ABI coverage

References are pinned to the upstream commit inspected, not a mutable branch.

| Protocol | Official reference | Result / boundary |
| --- | --- | --- |
| PumpFun | [IDL](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/idl/pump.json) | Checked native buy/sell and V2 discriminators, ordered accounts and arguments. Added explicit partial-fill flag. V2 quote token programs/ATAs and sharing config are present. |
| PumpSwap | [IDL](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/idl/pump_amm.json), [fee-recipient migration](https://github.com/pump-fun/pump-public-docs/blob/8cda1fa30ea658b20909d8aedf002047119388d2/docs/BREAKING_FEE_RECIPIENT.md) | Base accounts and buy/sell arguments match; existing builders already append pool-v2, buyback recipient/quote ATA and cashback accounts. Fee schedules and signed offsets must come from current snapshots. |
| Raydium LaunchLab (Bonk/StonkFun) | [builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/launchpad/instrument.ts) | 32-byte exact-in payload and 18-account zero-share-fee layout match, including platform/creator claim vaults. Arbitrary share-fee receiver and graduation partial fills are not enabled by this router. |
| Raydium CPMM | [builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/cpmm/instruction.ts) | Swap input/output layouts retain 13 accounts and 24-byte payloads. The recent creator-fee protocol-share change concerns collection account lists; this router builds swaps, not collections. Updated dependencies supply current config decoding. |
| Raydium AMM v4 | [V2 builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/liquidity/instruction.ts), [processor](https://github.com/raydium-io/raydium-amm/blob/d26944bfb76fb5fa8f91e5d440c2050ed358ef81/program/src/processor.rs) | V2 tags 16/17 and 8-account layout match. Fixed input fee fraction. Official builder uses SPL Token; Token-2022 support must not be inferred from a configurable program pubkey. Swap events alone do not carry authoritative fee fractions; refresh account state before quoting a custom-fee pool. |
| Raydium CLMM | [swap_v2 builder](https://github.com/raydium-io/raydium-sdk-V2/blob/cc33ec28a8921a35609e83293e9e07ad830b0779/src/raydium/clmm/instrument.ts) | 41-byte payload, 13 fixed accounts, optional bitmap extension then tick arrays match. Requires current tick state and an amount-matched quote. |
| Orca Whirlpool | [generated swap_v2](https://github.com/orca-so/whirlpools/blob/f2a3d13fa04eb15cf5b5a309ef9b226fd5d34e36/rust-sdk/client/src/generated/instructions/swap_v2.rs) | 15 fixed accounts and 43-byte payload with `remaining_accounts_info=None` match. Transfer hooks and supplemental tick arrays need additional remaining-account encoding, which this builder does not supply. |
| Meteora DLMM | [IDL](https://github.com/MeteoraAg/dlmm-sdk/blob/576919e3e4368e542c402f000b4264724f7f23ec/idls/dlmm.json) | swap2 discriminator, 16 fixed accounts and 28-byte payload with empty remaining-account slices match. Bin arrays follow fixed accounts; transfer-hook slices are not encoded. |
| Meteora DAMM v2 | [swap context](https://github.com/MeteoraAg/damm-v2/blob/6cd2614ff61206b456961d0704446da83be5c77a/programs/cp-amm/src/instructions/swap/ix_swap.rs) | swap2 argument struct is two u64s plus mode; optional referral sentinel/event accounts are retained. Existing instructions sysvar option must match the pool's rate-limiter requirements. |

Token-2022 program selection alone does not imply extension support. Transfer
fees require current epoch/mint schedules; transfer hooks require additional
accounts. Concentrated-liquidity quotes require current ticks/bins and direction,
not merely reserves or an earlier event's output. Dynamic following legs still
accept only their deliberately supported instruction layouts; upgrading a DEX
must not bypass the router's mint or budget guards.

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

155 SDK tests, 14 program tests and 2 example tests pass with upgraded pins.
SDK tests include the new non-default AMM fee fraction/inverse checks and signed
PumpSwap quote cases; network-gated tests are disabled in this run. Workspace
compilation is checked separately. The chain processor has not changed in this
supplement; its six SBF/LiteSVM security cases were verified in the preceding
security fix. Source changes have not upgraded the mainnet router deployment.
