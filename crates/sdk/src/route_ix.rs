//! Encode `Route` instruction for the on-chain Pinocchio router.

use anyhow::{bail, Result};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

use crate::{
    config_pda,
    constants::{
        BUY_EXACT_IN_LAUNCHLAB, CLMM_SWAP_V2, CPMM_SWAP_BASE_IN, LAUNCHLAB_PROGRAM,
        METEORA_DAMM_V2_EXACT_IN, METEORA_DAMM_V2_PROGRAM, METEORA_DAMM_V2_SWAP2,
        METEORA_DLMM_PROGRAM, METEORA_DLMM_SWAP2, ORCA_WHIRLPOOL_PROGRAM, PUMPSWAP_PROGRAM,
        PUMPSWAP_SELL, RAYDIUM_AMM_V4_PROGRAM, RAYDIUM_AMM_V4_SWAP_BASE_IN_V2,
        RAYDIUM_CLMM_PROGRAM, RAYDIUM_CPMM_PROGRAM, SYSTEM_PROGRAM, TOKEN_2022_PROGRAM,
        TOKEN_PROGRAM, WHIRLPOOL_SWAP_V2,
    },
    legs::Leg,
};

pub const TAG_ROUTE: u8 = 2;
/// Idempotent Pump creator-vault rent top-up, before route budget measurement.
pub const TAG_PREPARE_PUMPFUN: u8 = 7;
// Tags 3/4 had no expected output mint; upgraded programs reject those formats.
pub const TAG_ROUTE_DYNAMIC: u8 = 5;
/// Dynamic three-hop route: each later leg spends the previous leg's actual new output.
pub const TAG_ROUTE_DYNAMIC_THREE: u8 = 6;
pub const FEE_ASSET_SOL: u8 = 0;
pub const FEE_ASSET_TOKEN: u8 = 1;
/// OR into `fee_asset`: `amount_in` is a max budget (exact-out). On-chain checks
/// `fee <= spent <= amount_in` instead of `spent == amount_in`.
pub const FEE_ASSET_EXACT_OUT: u8 = 0x80;

#[derive(Clone, Copy)]
struct DynamicLegLayout {
    input_account: usize,
    output_account: usize,
    signer: usize,
    amount_offset: usize,
    minimum_accounts: usize,
}

fn whirlpool_dynamic_accounts(data: &[u8]) -> Option<usize> {
    if data.len() == 43 && data[42] == 0 {
        Some(15)
    } else if data.len() == 49 && data[42..48] == [1, 1, 0, 0, 0, 6]
        && (1..=3).contains(&data[48]) {
        Some(15 + data[48] as usize)
    } else {
        None
    }
}

fn dynamic_leg_layout(leg: &Leg) -> Result<DynamicLegLayout> {
    let data = &leg.data;
    let (input_account, output_account, signer, amount_offset, minimum_accounts) = if leg.program_id
        == LAUNCHLAB_PROGRAM
        && data.len() == 32
        && data[..8] == BUY_EXACT_IN_LAUNCHLAB
        && data[24..32] == [0u8; 8]
    {
        (6, 5, 0, 8, 13)
    } else if leg.program_id == RAYDIUM_CPMM_PROGRAM
        && data.len() == 24
        && data[..8] == CPMM_SWAP_BASE_IN
    {
        (4, 5, 0, 8, 12)
    } else if leg.program_id == PUMPSWAP_PROGRAM && data.len() == 24 && data[..8] == PUMPSWAP_SELL {
        // PumpSwap sell: user base -> user quote, with the exact base input at offset 8.
        (5, 6, 1, 8, 13)
    } else if leg.program_id == RAYDIUM_AMM_V4_PROGRAM
        && data.len() == 17
        && data[0] == RAYDIUM_AMM_V4_SWAP_BASE_IN_V2
    {
        (5, 6, 7, 1, 8)
    } else if leg.program_id == RAYDIUM_CLMM_PROGRAM
        && data.len() == 41
        && data[..8] == CLMM_SWAP_V2
        && data[40] == 1
    {
        (3, 4, 0, 8, 13)
    } else if leg.program_id == METEORA_DLMM_PROGRAM
        && data.len() == 28
        && data[..8] == METEORA_DLMM_SWAP2
        && data[24..28] == [0u8; 4]
    {
        (4, 5, 10, 8, 13)
    } else if leg.program_id == METEORA_DAMM_V2_PROGRAM
        && data.len() == 25
        && data[..8] == METEORA_DAMM_V2_SWAP2
        && data[24] == METEORA_DAMM_V2_EXACT_IN
    {
        (2, 3, 8, 8, 11)
    } else if leg.program_id == ORCA_WHIRLPOOL_PROGRAM
        && whirlpool_dynamic_accounts(data).is_some()
        && data[..8] == WHIRLPOOL_SWAP_V2
        && data[40] == 1
        && data[41] <= 1
    {
        if data[41] == 1 {
            (7, 9, 3, 8, whirlpool_dynamic_accounts(data).unwrap())
        } else {
            (9, 7, 3, 8, whirlpool_dynamic_accounts(data).unwrap())
        }
    } else {
        bail!("dynamic leg must be a supported exact-input swap");
    };
    Ok(DynamicLegLayout {
        input_account,
        output_account,
        signer,
        amount_offset,
        minimum_accounts,
    })
}

fn validate_dynamic_leg(
    leg: &Leg,
    payer: Pubkey,
    input: Pubkey,
    output: Pubkey,
    require_zero_amount: bool,
    allow_launchlab: bool,
) -> Result<()> {
    if !allow_launchlab && leg.program_id == LAUNCHLAB_PROGRAM {
        bail!("LaunchLab is only supported as the dynamic final leg");
    }
    let layout = dynamic_leg_layout(leg)?;
    if leg.accounts.len() < layout.minimum_accounts
        || leg.accounts[layout.signer].pubkey != payer
        || !leg.accounts[layout.signer].is_signer
        || leg.accounts[layout.input_account].pubkey != input
        || !leg.accounts[layout.input_account].is_writable
        || leg.accounts[layout.output_account].pubkey != output
        || !leg.accounts[layout.output_account].is_writable
        || (require_zero_amount
            && leg.data[layout.amount_offset..layout.amount_offset + 8] != [0u8; 8])
    {
        bail!("dynamic leg accounts or input amount do not match the route");
    }
    Ok(())
}

pub struct RouteAccounts {
    pub payer: Pubkey,
    pub fee_destination: Pubkey,
    pub fee_source: Pubkey,
    /// Token account whose balance delta is checked against `min_amount_out`,
    /// or payer for native SOL with the System Program output-mint sentinel.
    pub output_token_account: Pubkey,
    /// System program for SOL fee, or SPL Token / Token-2022 for token fee.
    pub fee_program: Pubkey,
    /// Fee-source mint. Token-2022 routes append it after the leg/program metas
    /// for checked fee transfers, without moving any existing account index.
    pub fee_mint: Pubkey,
}

pub fn build_route_instruction(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    amount_in: u64,
    min_amount_out: u64,
    fee_asset: u8,
    expected_output_mint: &Pubkey,
    legs: &[Leg],
) -> Instruction {
    build_route_instruction_ex(
        program_id,
        accounts,
        amount_in,
        min_amount_out,
        fee_asset,
        false,
        expected_output_mint,
        legs,
    )
}

/// Same as [`build_route_instruction`] with an explicit exact-out fee check flag.
pub fn build_route_instruction_ex(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    amount_in: u64,
    min_amount_out: u64,
    fee_asset: u8,
    exact_out: bool,
    expected_output_mint: &Pubkey,
    legs: &[Leg],
) -> Instruction {
    let (config, _) = config_pda(program_id);
    let fee_asset_byte = if exact_out {
        (fee_asset & 0x01) | FEE_ASSET_EXACT_OUT
    } else {
        fee_asset & 0x01
    };

    let mut data = Vec::with_capacity(64 + legs.len() * 48);
    data.push(TAG_ROUTE);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data.push(fee_asset_byte);
    data.push(legs.len() as u8);
    data.extend_from_slice(expected_output_mint.as_ref());

    // Account layout must match on-chain `route::process` (6 fixed + remaining).
    let mut metas = Vec::with_capacity(6 + 32);
    metas.push(AccountMeta::new(accounts.payer, true));
    metas.push(AccountMeta::new_readonly(config, false));
    metas.push(AccountMeta::new(accounts.fee_destination, false));
    metas.push(AccountMeta::new(accounts.fee_source, false));
    metas.push(AccountMeta::new(accounts.output_token_account, false));
    metas.push(AccountMeta::new_readonly(accounts.fee_program, false));

    for leg in legs {
        data.extend_from_slice(leg.program_id.as_ref());
        data.push(leg.accounts.len() as u8);
        data.extend_from_slice(&(leg.data.len() as u16).to_le_bytes());
        data.extend_from_slice(&leg.data);
        metas.extend(leg.accounts.iter().cloned());
    }

    let mut seen = [Pubkey::default(); 4];
    let mut seen_n = 0usize;
    for leg in legs {
        let already = seen[..seen_n].iter().any(|p| p == &leg.program_id);
        if already {
            continue;
        }
        metas.push(AccountMeta::new_readonly(leg.program_id, false));
        if seen_n < seen.len() {
            seen[seen_n] = leg.program_id;
            seen_n += 1;
        }
    }

    append_fee_mint(&mut metas, &accounts);
    Instruction {
        program_id: *program_id,
        accounts: metas,
        data,
    }
}

/// Build an exact-input route that spends the *actual newly received* bridge
/// tokens in leg two. Leg one may use any supported CPI builder; leg two must
/// be LaunchLab buyExactIn or a supported exact-input pool swap. The original Route
/// format and its fixed-input behavior remain unchanged.
///
/// The caller must provide fresh pool accounts, construct leg one with the
/// intended input amount, and include ATA/WSOL setup before this instruction.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamic_route_instruction(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    intermediate_token_account: Pubkey,
    amount_in: u64,
    intermediate_min_out: u64,
    min_amount_out: u64,
    expected_output_mint: &Pubkey,
    first_leg: &Leg,
    second_leg: &Leg,
) -> Result<Instruction> {
    if amount_in == 0 || intermediate_min_out == 0 || min_amount_out == 0 {
        bail!("dynamic route amounts must be positive");
    }
    if *expected_output_mint == SYSTEM_PROGRAM {
        bail!("dynamic route output must be a token mint; use WSOL for SOL settlement");
    }
    if intermediate_token_account == accounts.fee_source
        || intermediate_token_account == accounts.output_token_account
    {
        bail!("intermediate token account must differ from source and output");
    }
    if !first_leg
        .accounts
        .iter()
        .any(|meta| meta.pubkey == intermediate_token_account && meta.is_writable)
    {
        bail!("first leg must write to the intermediate token account");
    }
    if first_leg.accounts.is_empty()
        || first_leg.accounts.len() > 64
        || second_leg.accounts.len() > 64
    {
        bail!("dynamic route leg account count is invalid");
    }
    validate_dynamic_leg(
        second_leg,
        accounts.payer,
        intermediate_token_account,
        accounts.output_token_account,
        false,
        true,
    )?;
    if first_leg.data.len() > u16::MAX as usize || second_leg.data.len() > u16::MAX as usize {
        bail!("dynamic route leg instruction data is too long");
    }

    let (config, _) = config_pda(program_id);
    let mut data = Vec::with_capacity(59 + 2 * 35 + first_leg.data.len() + second_leg.data.len());
    data.push(TAG_ROUTE_DYNAMIC);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data.push(FEE_ASSET_TOKEN); // native SOL is wrapped to WSOL in setup
    data.push(2);
    data.extend_from_slice(expected_output_mint.as_ref());
    data.extend_from_slice(&intermediate_min_out.to_le_bytes());

    let mut metas =
        Vec::with_capacity(7 + first_leg.accounts.len() + second_leg.accounts.len() + 2);
    metas.push(AccountMeta::new(accounts.payer, true));
    metas.push(AccountMeta::new_readonly(config, false));
    metas.push(AccountMeta::new(accounts.fee_destination, false));
    metas.push(AccountMeta::new(accounts.fee_source, false));
    metas.push(AccountMeta::new(accounts.output_token_account, false));
    metas.push(AccountMeta::new_readonly(accounts.fee_program, false));
    metas.push(AccountMeta::new(intermediate_token_account, false));

    for leg in [first_leg, second_leg] {
        data.extend_from_slice(leg.program_id.as_ref());
        data.push(leg.accounts.len() as u8);
        data.extend_from_slice(&(leg.data.len() as u16).to_le_bytes());
        data.extend_from_slice(&leg.data);
        metas.extend(leg.accounts.iter().cloned());
    }
    metas.push(AccountMeta::new_readonly(first_leg.program_id, false));
    if second_leg.program_id != first_leg.program_id {
        metas.push(AccountMeta::new_readonly(second_leg.program_id, false));
    }
    append_fee_mint(&mut metas, &accounts);
    Ok(Instruction {
        program_id: *program_id,
        accounts: metas,
        data,
    })
}

/// Build a three-hop route. `spend_actual_output=false` preserves the fixed-input
/// tag-2 format; each leg must already encode its intended input amount and
/// intermediate minimum output. In that mode `first_min_out` and
/// `second_min_out` are not inserted into the route instruction.
/// `true` emits tag 6 and replaces the zero input in both later exact-input
/// legs with the previous leg's newly credited balance.
/// Both intermediate accounts must be user-owned, writable token accounts.
/// The caller remains responsible for constructing the first leg (which can be
/// a raw [`Leg`]), preparing token accounts, and supplying fresh pool accounts.
#[allow(clippy::too_many_arguments)]
pub fn build_three_hop_route_instruction(
    program_id: &Pubkey,
    accounts: RouteAccounts,
    first_intermediate: Pubkey,
    second_intermediate: Pubkey,
    amount_in: u64,
    first_min_out: u64,
    second_min_out: u64,
    min_amount_out: u64,
    expected_output_mint: &Pubkey,
    legs: &[Leg],
    spend_actual_output: bool,
) -> Result<Instruction> {
    if legs.len() != 3 {
        bail!("three-hop route requires exactly three legs");
    }
    if !spend_actual_output {
        return Ok(build_route_instruction_ex(
            program_id,
            accounts,
            amount_in,
            min_amount_out,
            FEE_ASSET_TOKEN,
            false,
            expected_output_mint,
            legs,
        ));
    }
    if amount_in == 0 || first_min_out == 0 || second_min_out == 0 || min_amount_out == 0 {
        bail!("dynamic three-hop route amounts must be positive");
    }
    if *expected_output_mint == SYSTEM_PROGRAM {
        bail!("dynamic route output must be a token mint; use WSOL for SOL settlement");
    }
    if first_intermediate == second_intermediate
        || first_intermediate == accounts.fee_source
        || first_intermediate == accounts.output_token_account
        || second_intermediate == accounts.fee_source
        || second_intermediate == accounts.output_token_account
    {
        bail!("dynamic intermediate token accounts must be distinct from source and output");
    }
    for leg in legs {
        if leg.accounts.is_empty()
            || leg.accounts.len() > 64
            || leg.data.is_empty()
            || leg.data.len() > u16::MAX as usize
        {
            bail!("dynamic three-hop leg account count or data length is invalid");
        }
    }
    if !legs[0]
        .accounts
        .iter()
        .any(|meta| meta.pubkey == first_intermediate && meta.is_writable)
        || legs[0]
            .accounts
            .iter()
            .any(|meta| meta.pubkey == second_intermediate && meta.is_writable)
    {
        bail!("first leg must write the first intermediate but not the second intermediate");
    }
    validate_dynamic_leg(
        &legs[1],
        accounts.payer,
        first_intermediate,
        second_intermediate,
        true,
        false,
    )?;
    validate_dynamic_leg(
        &legs[2],
        accounts.payer,
        second_intermediate,
        accounts.output_token_account,
        true,
        true,
    )?;

    let (config, _) = config_pda(program_id);
    let mut data =
        Vec::with_capacity(67 + 3 * 35 + legs.iter().map(|leg| leg.data.len()).sum::<usize>());
    data.push(TAG_ROUTE_DYNAMIC_THREE);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data.push(FEE_ASSET_TOKEN);
    data.push(3);
    data.extend_from_slice(expected_output_mint.as_ref());
    data.extend_from_slice(&first_min_out.to_le_bytes());
    data.extend_from_slice(&second_min_out.to_le_bytes());

    let mut metas =
        Vec::with_capacity(8 + legs.iter().map(|leg| leg.accounts.len()).sum::<usize>() + 3);
    metas.push(AccountMeta::new(accounts.payer, true));
    metas.push(AccountMeta::new_readonly(config, false));
    metas.push(AccountMeta::new(accounts.fee_destination, false));
    metas.push(AccountMeta::new(accounts.fee_source, false));
    metas.push(AccountMeta::new(accounts.output_token_account, false));
    metas.push(AccountMeta::new_readonly(accounts.fee_program, false));
    metas.push(AccountMeta::new(first_intermediate, false));
    metas.push(AccountMeta::new(second_intermediate, false));

    for leg in legs {
        data.extend_from_slice(leg.program_id.as_ref());
        data.push(leg.accounts.len() as u8);
        data.extend_from_slice(&(leg.data.len() as u16).to_le_bytes());
        data.extend_from_slice(&leg.data);
        metas.extend(leg.accounts.iter().cloned());
    }
    let mut seen = [Pubkey::default(); 3];
    let mut seen_n = 0;
    for leg in legs {
        if seen[..seen_n].contains(&leg.program_id) {
            continue;
        }
        metas.push(AccountMeta::new_readonly(leg.program_id, false));
        seen[seen_n] = leg.program_id;
        seen_n += 1;
    }
    append_fee_mint(&mut metas, &accounts);
    Ok(Instruction {
        program_id: *program_id,
        accounts: metas,
        data,
    })
}

fn append_fee_mint(metas: &mut Vec<AccountMeta>, accounts: &RouteAccounts) {
    if accounts.fee_program == TOKEN_2022_PROGRAM
        && !metas.iter().any(|meta| meta.pubkey == accounts.fee_mint)
    {
        metas.push(AccountMeta::new_readonly(accounts.fee_mint, false));
    }
}

pub fn sol_fee_program() -> Pubkey {
    SYSTEM_PROGRAM
}

pub fn spl_fee_program() -> Pubkey {
    TOKEN_PROGRAM
}

/// Pick Token vs Token-2022 for fee CPI based on the fee_source ATA's token program.
pub fn token_fee_program(token_program: &Pubkey) -> Pubkey {
    if *token_program == TOKEN_2022_PROGRAM {
        TOKEN_2022_PROGRAM
    } else {
        TOKEN_PROGRAM
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        METEORA_DAMM_V2_PROGRAM, METEORA_DLMM_PROGRAM, ORCA_WHIRLPOOL_PROGRAM, PUMPSWAP_PROGRAM,
        RAYDIUM_AMM_V4_PROGRAM, RAYDIUM_CLMM_PROGRAM, SELL_EXACT_IN_LAUNCHLAB, WSOL_MINT,
    };

    fn dynamic_pool_leg(
        program_id: Pubkey,
        payer: Pubkey,
        input: Pubkey,
        output: Pubkey,
        a_to_b: bool,
    ) -> Leg {
        let data = if program_id == RAYDIUM_AMM_V4_PROGRAM {
            let mut bytes = vec![0u8; 17];
            bytes[0] = RAYDIUM_AMM_V4_SWAP_BASE_IN_V2;
            bytes
        } else if program_id == PUMPSWAP_PROGRAM {
            let mut bytes = vec![0u8; 24];
            bytes[..8].copy_from_slice(&PUMPSWAP_SELL);
            bytes
        } else if program_id == RAYDIUM_CLMM_PROGRAM {
            let mut bytes = vec![0u8; 41];
            bytes[..8].copy_from_slice(&CLMM_SWAP_V2);
            bytes[40] = 1;
            bytes
        } else if program_id == METEORA_DLMM_PROGRAM {
            let mut bytes = vec![0u8; 28];
            bytes[..8].copy_from_slice(&METEORA_DLMM_SWAP2);
            bytes
        } else if program_id == METEORA_DAMM_V2_PROGRAM {
            let mut bytes = vec![0u8; 25];
            bytes[..8].copy_from_slice(&METEORA_DAMM_V2_SWAP2);
            bytes
        } else if program_id == ORCA_WHIRLPOOL_PROGRAM {
            let mut bytes = vec![0u8; 43];
            bytes[..8].copy_from_slice(&WHIRLPOOL_SWAP_V2);
            bytes[40] = 1;
            bytes[41] = u8::from(a_to_b);
            bytes
        } else {
            let mut bytes = vec![0u8; 24];
            bytes[..8].copy_from_slice(&CPMM_SWAP_BASE_IN);
            bytes
        };
        let mut leg = Leg {
            program_id,
            accounts: Vec::new(),
            data,
        };
        let layout = dynamic_leg_layout(&leg).unwrap();
        leg.accounts =
            vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); layout.minimum_accounts];
        leg.accounts[layout.signer] = AccountMeta::new_readonly(payer, true);
        leg.accounts[layout.input_account] = AccountMeta::new(input, false);
        leg.accounts[layout.output_account] = AccountMeta::new(output, false);
        leg
    }

    #[test]
    fn dynamic_route_accepts_any_first_dex_and_both_supported_targets() {
        let payer = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let output = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let route_accounts = || RouteAccounts {
            payer,
            fee_destination: Pubkey::new_unique(),
            fee_source: source,
            output_token_account: output,
            fee_program: TOKEN_PROGRAM,
            fee_mint: Pubkey::new_unique(),
        };
        let mut launch_data = vec![0u8; 32];
        launch_data[..8].copy_from_slice(&BUY_EXACT_IN_LAUNCHLAB);
        let mut cpmm_data = vec![0u8; 24];
        cpmm_data[..8].copy_from_slice(&CPMM_SWAP_BASE_IN);
        let launch = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: {
                let mut metas = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 18];
                metas[0] = AccountMeta::new_readonly(payer, true);
                metas[5] = AccountMeta::new(output, false);
                metas[6] = AccountMeta::new(quote, false);
                metas
            },
            data: launch_data,
        };
        let cpmm = Leg {
            program_id: RAYDIUM_CPMM_PROGRAM,
            accounts: {
                let mut metas = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 13];
                metas[0] = AccountMeta::new_readonly(payer, true);
                metas[4] = AccountMeta::new(quote, false);
                metas[5] = AccountMeta::new(output, false);
                metas
            },
            data: cpmm_data,
        };
        for first_program in [
            METEORA_DLMM_PROGRAM,
            METEORA_DAMM_V2_PROGRAM,
            RAYDIUM_AMM_V4_PROGRAM,
            RAYDIUM_CPMM_PROGRAM,
            RAYDIUM_CLMM_PROGRAM,
            ORCA_WHIRLPOOL_PROGRAM,
            PUMPSWAP_PROGRAM,
        ] {
            let first = Leg {
                program_id: first_program,
                accounts: vec![AccountMeta::new(quote, false)],
                data: vec![1],
            };
            for second in [&launch, &cpmm] {
                let ix = build_dynamic_route_instruction(
                    &crate::PROGRAM_ID,
                    route_accounts(),
                    quote,
                    100,
                    80,
                    7,
                    &Pubkey::new_unique(),
                    &first,
                    second,
                )
                .unwrap();
                assert_eq!(ix.data[0], TAG_ROUTE_DYNAMIC);
                assert_eq!(ix.data[18], 2);
                assert_eq!(&ix.data[51..59], &80u64.to_le_bytes());
                assert_eq!(ix.accounts[6].pubkey, quote);
                assert_eq!(ix.accounts[7].pubkey, quote);
                assert_eq!(&ix.data[59..91], first_program.as_ref());
                let mut token_2022_accounts = route_accounts();
                token_2022_accounts.fee_program = TOKEN_2022_PROGRAM;
                let fee_mint = token_2022_accounts.fee_mint;
                let checked = build_dynamic_route_instruction(
                    &crate::PROGRAM_ID, token_2022_accounts, quote,
                    100, 80, 7, &Pubkey::new_unique(), &first, second,
                ).unwrap();
                assert_eq!(checked.accounts[7].pubkey, quote);
                assert_eq!(checked.accounts.last(), Some(&AccountMeta::new_readonly(fee_mint, false)));
            }
        }
    }

    #[test]
    fn dynamic_route_rejects_unconnected_or_wrong_second_leg() {
        let payer = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let output = Pubkey::new_unique();
        let first = Leg {
            program_id: METEORA_DLMM_PROGRAM,
            accounts: vec![AccountMeta::new(quote, false)],
            data: vec![1],
        };
        let second = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: vec![AccountMeta::new(payer, true); 7],
            data: vec![0; 32],
        };
        let accounts = RouteAccounts {
            payer,
            fee_destination: Pubkey::new_unique(),
            fee_source: Pubkey::new_unique(),
            output_token_account: output,
            fee_program: TOKEN_PROGRAM,
            fee_mint: Pubkey::new_unique(),
        };
        assert!(build_dynamic_route_instruction(
            &crate::PROGRAM_ID,
            accounts,
            quote,
            100,
            80,
            7,
            &Pubkey::new_unique(),
            &first,
            &second
        )
        .is_err());
    }

    #[test]
    fn pumpswap_dynamic_following_leg_accepts_sell_but_not_other_instructions() {
        let payer = Pubkey::new_unique();
        let input = Pubkey::new_unique();
        let output = Pubkey::new_unique();
        let mut sell = dynamic_pool_leg(PUMPSWAP_PROGRAM, payer, input, output, false);
        let layout = dynamic_leg_layout(&sell).unwrap();
        assert_eq!(
            (layout.input_account, layout.output_account, layout.signer),
            (5, 6, 1)
        );
        validate_dynamic_leg(&sell, payer, input, output, true, false).unwrap();

        sell.data[8..16].copy_from_slice(&1u64.to_le_bytes());
        assert!(validate_dynamic_leg(&sell, payer, input, output, true, false).is_err());
        validate_dynamic_leg(&sell, payer, input, output, false, false).unwrap();

        sell.data[8..16].fill(0);
        sell.accounts[5] = AccountMeta::new(output, false);
        assert!(validate_dynamic_leg(&sell, payer, input, output, true, false).is_err());
        sell.accounts[5] = AccountMeta::new(input, false);
        sell.data[0] ^= 1;
        assert!(dynamic_leg_layout(&sell).is_err());
        sell.data[0] ^= 1;
        sell.data.pop();
        assert!(dynamic_leg_layout(&sell).is_err());
    }

    #[test]
    fn pumpswap_sell_builder_matches_dynamic_account_layout() {
        let payer = Pubkey::new_unique();
        let base = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let pool = crate::market::PumpSwapPool {
            pool: Pubkey::new_unique(),
            base_mint: base,
            quote_mint: quote,
            pool_base_token_account: Pubkey::new_unique(),
            pool_quote_token_account: Pubkey::new_unique(),
            base_token_program: TOKEN_PROGRAM,
            quote_token_program: TOKEN_PROGRAM,
            coin_creator_vault_ata: Pubkey::new_unique(),
            coin_creator_vault_authority: Pubkey::new_unique(),
            coin_creator: Pubkey::default(),
            base_reserve: 0,
            quote_reserve: 0,
            virtual_quote_reserves: 0,
            lp_fee_bps: 0,
            protocol_fee_bps: 95,
            creator_fee_bps: 0,
            is_cashback_coin: false,
            protocol_fee_recipient: Pubkey::new_unique(),
            buyback_fee_recipient: Pubkey::new_unique(),
        };
        let leg = crate::legs::pumpswap_sell_leg(&payer, &pool, 0, 1).unwrap();
        let input = crate::ata::ata(&payer, &base, &TOKEN_PROGRAM);
        let output = crate::ata::ata(&payer, &quote, &TOKEN_PROGRAM);
        validate_dynamic_leg(&leg, payer, input, output, true, false).unwrap();
        assert_eq!(leg.accounts[3].pubkey, base);
        assert_eq!(leg.accounts[4].pubkey, quote);
        assert_eq!(leg.accounts[11].pubkey, TOKEN_PROGRAM);
        assert_eq!(leg.accounts[12].pubkey, TOKEN_PROGRAM);
    }

    #[test]
    fn legacy_route_tag_and_account_layout_remain_unchanged() {
        let payer = Pubkey::new_unique();
        let first_account = Pubkey::new_unique();
        let first = Leg {
            program_id: Pubkey::new_from_array([3; 32]),
            accounts: vec![AccountMeta::new(first_account, false)],
            data: vec![1, 2, 3],
        };
        let ix = build_route_instruction(
            &crate::PROGRAM_ID,
            RouteAccounts {
                payer,
                fee_destination: Pubkey::new_unique(),
                fee_source: Pubkey::new_unique(),
                output_token_account: Pubkey::new_unique(),
                fee_program: TOKEN_PROGRAM,
                fee_mint: WSOL_MINT,
            },
            100,
            10,
            FEE_ASSET_TOKEN,
            &Pubkey::new_from_array([5; 32]),
            &[first.clone()],
        );
        // Wire fixture for baseline 472d57c: input=100, minimum=10,
        // fee_asset=1, output mint=[5;32], one leg ([3;32], one account, [1,2,3]).
        let golden = "0264000000000000000a00000000000000010105050505050505050505050505050505050505050505050505050505050505050303030303030303030303030303030303030303030303030303030303030303010300010203";
        let actual: String = ix.data.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(actual, golden);
        assert_eq!(ix.data[0], TAG_ROUTE);
        assert_eq!(ix.data[18], 1);
        assert_eq!(ix.accounts[6].pubkey, first_account); // no extra intermediate account
        let mut token_2022 = build_route_instruction(
            &crate::PROGRAM_ID,
            RouteAccounts {
                payer,
                fee_destination: ix.accounts[2].pubkey,
                fee_source: ix.accounts[3].pubkey,
                output_token_account: ix.accounts[4].pubkey,
                fee_program: TOKEN_2022_PROGRAM,
                fee_mint: WSOL_MINT,
            },
            100, 10, FEE_ASSET_TOKEN, &Pubkey::new_from_array([5; 32]), &[first],
        );
        assert_eq!(token_2022.data, ix.data);
        assert_eq!(token_2022.accounts.pop(), Some(AccountMeta::new_readonly(WSOL_MINT, false)));
        token_2022.accounts[5] = ix.accounts[5].clone();
        assert_eq!(token_2022.accounts, ix.accounts);
    }

    #[test]
    fn reverse_two_hop_launchlab_sell_can_finish_on_each_supported_pool() {
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let mut sell_data = vec![0u8; 32];
        sell_data[..8].copy_from_slice(&SELL_EXACT_IN_LAUNCHLAB);
        sell_data[8..16].copy_from_slice(&100u64.to_le_bytes());
        let first = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: vec![
                AccountMeta::new_readonly(payer, true),
                AccountMeta::new(mint, false),
                AccountMeta::new(quote, false),
            ],
            data: sell_data,
        };
        for program in [
            RAYDIUM_AMM_V4_PROGRAM,
            RAYDIUM_CPMM_PROGRAM,
            RAYDIUM_CLMM_PROGRAM,
            METEORA_DLMM_PROGRAM,
            METEORA_DAMM_V2_PROGRAM,
            ORCA_WHIRLPOOL_PROGRAM,
            PUMPSWAP_PROGRAM,
        ] {
            let last = dynamic_pool_leg(program, payer, quote, wsol, true);
            let ix = build_dynamic_route_instruction(
                &crate::PROGRAM_ID,
                RouteAccounts {
                    payer,
                    fee_destination: Pubkey::new_unique(),
                    fee_source: mint,
                    output_token_account: wsol,
                    fee_program: TOKEN_PROGRAM,
                    fee_mint: Pubkey::new_unique(),
                },
                quote,
                100,
                1,
                1,
                &Pubkey::new_unique(),
                &first,
                &last,
            )
            .unwrap();
            assert_eq!(ix.data[0], TAG_ROUTE_DYNAMIC);
            assert_eq!(ix.accounts[6].pubkey, quote);
        }
    }

    #[test]
    fn three_hop_buy_and_sell_accept_each_supported_usdc_quote_pool() {
        let payer = Pubkey::new_unique();
        let wsol = Pubkey::new_unique();
        let usdc = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let mut buy_data = vec![0u8; 32];
        buy_data[..8].copy_from_slice(&BUY_EXACT_IN_LAUNCHLAB);
        let mut buy = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 18],
            data: buy_data,
        };
        buy.accounts[0] = AccountMeta::new_readonly(payer, true);
        buy.accounts[5] = AccountMeta::new(mint, false);
        buy.accounts[6] = AccountMeta::new(quote, false);
        let mut sell_data = vec![0u8; 32];
        sell_data[..8].copy_from_slice(&SELL_EXACT_IN_LAUNCHLAB);
        let sell = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: vec![
                AccountMeta::new_readonly(payer, true),
                AccountMeta::new(quote, false),
            ],
            data: sell_data,
        };
        let wsol_to_usdc = dynamic_pool_leg(RAYDIUM_CPMM_PROGRAM, payer, wsol, usdc, true);
        for program in [
            RAYDIUM_AMM_V4_PROGRAM,
            RAYDIUM_CPMM_PROGRAM,
            RAYDIUM_CLMM_PROGRAM,
            METEORA_DLMM_PROGRAM,
            METEORA_DAMM_V2_PROGRAM,
            ORCA_WHIRLPOOL_PROGRAM,
            PUMPSWAP_PROGRAM,
        ] {
            let buy_middle = dynamic_pool_leg(program, payer, usdc, quote, true);
            let buy_ix = build_three_hop_route_instruction(
                &crate::PROGRAM_ID,
                RouteAccounts {
                    payer,
                    fee_destination: Pubkey::new_unique(),
                    fee_source: wsol,
                    output_token_account: mint,
                    fee_program: TOKEN_PROGRAM,
                    fee_mint: WSOL_MINT,
                },
                usdc,
                quote,
                100,
                1,
                1,
                1,
                &Pubkey::new_unique(),
                &[wsol_to_usdc.clone(), buy_middle, buy.clone()],
                true,
            )
            .unwrap();
            assert_eq!(buy_ix.data[0], TAG_ROUTE_DYNAMIC_THREE);

            let sell_middle = dynamic_pool_leg(program, payer, quote, usdc, false);
            for final_program in [
                RAYDIUM_AMM_V4_PROGRAM,
                RAYDIUM_CPMM_PROGRAM,
                RAYDIUM_CLMM_PROGRAM,
                METEORA_DLMM_PROGRAM,
                METEORA_DAMM_V2_PROGRAM,
                ORCA_WHIRLPOOL_PROGRAM,
                PUMPSWAP_PROGRAM,
            ] {
                let usdc_to_wsol = dynamic_pool_leg(final_program, payer, usdc, wsol, false);
                let sell_ix = build_three_hop_route_instruction(
                    &crate::PROGRAM_ID,
                    RouteAccounts {
                        payer,
                        fee_destination: Pubkey::new_unique(),
                        fee_source: mint,
                        output_token_account: wsol,
                        fee_program: TOKEN_PROGRAM,
                        fee_mint: Pubkey::new_unique(),
                    },
                    quote,
                    usdc,
                    100,
                    1,
                    1,
                    1,
                    &Pubkey::new_unique(),
                    &[sell.clone(), sell_middle.clone(), usdc_to_wsol],
                    true,
                )
                .unwrap();
                assert_eq!(sell_ix.data[0], TAG_ROUTE_DYNAMIC_THREE);
            }
        }
    }

    #[test]
    fn three_hop_mode_switch_preserves_fixed_route_and_encodes_dynamic_accounts() {
        let payer = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let usdc = Pubkey::new_unique();
        let quote = Pubkey::new_unique();
        let output = Pubkey::new_unique();
        let first_program = Pubkey::new_unique(); // raw first-hop CPI, not a built-in DEX
        let route_accounts = || RouteAccounts {
            payer,
            fee_destination: Pubkey::new_unique(),
            fee_source: source,
            output_token_account: output,
            fee_program: TOKEN_PROGRAM,
            fee_mint: WSOL_MINT,
        };
        let first = Leg {
            program_id: first_program,
            accounts: vec![AccountMeta::new(usdc, false)],
            data: vec![9],
        };
        let mut middle_accounts = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 16];
        middle_accounts[4] = AccountMeta::new(usdc, false);
        middle_accounts[5] = AccountMeta::new(quote, false);
        middle_accounts[10] = AccountMeta::new_readonly(payer, true);
        let mut middle_data = vec![0u8; 28];
        middle_data[..8].copy_from_slice(&METEORA_DLMM_SWAP2);
        middle_data[16..24].copy_from_slice(&300u64.to_le_bytes());
        let middle = Leg {
            program_id: METEORA_DLMM_PROGRAM,
            accounts: middle_accounts,
            data: middle_data,
        };
        let mut final_accounts = vec![AccountMeta::new_readonly(Pubkey::new_unique(), false); 18];
        final_accounts[0] = AccountMeta::new_readonly(payer, true);
        final_accounts[5] = AccountMeta::new(output, false);
        final_accounts[6] = AccountMeta::new(quote, false);
        let mut final_data = vec![0u8; 32];
        final_data[..8].copy_from_slice(&BUY_EXACT_IN_LAUNCHLAB);
        final_data[16..24].copy_from_slice(&100u64.to_le_bytes());
        let final_leg = Leg {
            program_id: LAUNCHLAB_PROGRAM,
            accounts: final_accounts,
            data: final_data,
        };
        let mut legs = [first, middle, final_leg];

        let mut fixed_legs = legs.clone();
        fixed_legs[1].data[8..16].copy_from_slice(&400u64.to_le_bytes());
        fixed_legs[2].data[8..16].copy_from_slice(&300u64.to_le_bytes());

        let fixed = build_three_hop_route_instruction(
            &crate::PROGRAM_ID,
            route_accounts(),
            usdc,
            quote,
            500,
            400,
            300,
            100,
            &Pubkey::new_unique(),
            &fixed_legs,
            false,
        )
        .unwrap();
        assert_eq!(fixed.data[0], TAG_ROUTE);
        assert_eq!(fixed.data[18], 3);
        assert_eq!(fixed.accounts[6].pubkey, usdc); // first leg account, not a header account
        assert_eq!(&fixed.data[130..138], &400u64.to_le_bytes());
        assert_eq!(&fixed.data[193..201], &300u64.to_le_bytes());

        let dynamic = build_three_hop_route_instruction(
            &crate::PROGRAM_ID,
            route_accounts(),
            usdc,
            quote,
            500,
            400,
            300,
            100,
            &Pubkey::new_unique(),
            &legs,
            true,
        )
        .unwrap();
        assert_eq!(dynamic.data[0], TAG_ROUTE_DYNAMIC_THREE);
        assert_eq!(dynamic.data[18], 3);
        assert_eq!(&dynamic.data[51..59], &400u64.to_le_bytes());
        assert_eq!(&dynamic.data[59..67], &300u64.to_le_bytes());
        assert_eq!(&dynamic.data[67..99], first_program.as_ref());
        assert_eq!(dynamic.accounts[6].pubkey, usdc);
        assert_eq!(dynamic.accounts[7].pubkey, quote);
        assert_eq!(dynamic.accounts[8].pubkey, usdc);
        let mut token_2022_accounts = route_accounts();
        token_2022_accounts.fee_program = TOKEN_2022_PROGRAM;
        let checked = build_three_hop_route_instruction(
            &crate::PROGRAM_ID, token_2022_accounts, usdc, quote,
            500, 400, 300, 100, &Pubkey::new_unique(), &legs, true,
        ).unwrap();
        assert_eq!(checked.accounts[8].pubkey, usdc);
        assert_eq!(checked.accounts.last(), Some(&AccountMeta::new_readonly(WSOL_MINT, false)));

        legs[0].accounts.push(AccountMeta::new(quote, false));
        assert!(build_three_hop_route_instruction(
            &crate::PROGRAM_ID,
            route_accounts(),
            usdc,
            quote,
            500,
            400,
            300,
            100,
            &Pubkey::new_unique(),
            &legs,
            true,
        )
        .is_err());
        legs[0].accounts.pop();

        legs[1].data[8] = 1;
        assert!(build_three_hop_route_instruction(
            &crate::PROGRAM_ID,
            route_accounts(),
            usdc,
            quote,
            500,
            400,
            300,
            100,
            &Pubkey::new_unique(),
            &legs,
            true,
        )
        .is_err());
        legs[1].data[8] = 0;
        legs[2].accounts[6] = AccountMeta::new(Pubkey::new_unique(), false);
        assert!(build_three_hop_route_instruction(
            &crate::PROGRAM_ID,
            route_accounts(),
            usdc,
            quote,
            500,
            400,
            300,
            100,
            &Pubkey::new_unique(),
            &legs,
            true,
        )
        .is_err());
    }
}
