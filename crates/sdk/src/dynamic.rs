//! Explicit-pool dynamic two-hop buys; no RPC or automatic pool discovery.

use anyhow::{anyhow, bail, Result};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use crate::{
    ata::ata,
    constants::{PROGRAM_ID, TOKEN_PROGRAM, WSOL_MINT},
    legs::{
        cpmm_swap_leg, launchlab_buy_leg, meteora_damm_v2_swap_leg, meteora_dlmm_swap_leg,
        pumpswap_buy_leg, pumpswap_sell_leg, raydium_amm_v4_swap_leg, raydium_clmm_swap_leg,
        require_mint_programs, whirlpool_swap_leg, Leg,
    },
    market::Market,
    route_ix::{build_dynamic_route_instruction, RouteAccounts},
};

/// The route instruction and the two user ATAs that must exist before it runs.
/// If paying native SOL, create/fund/sync the WSOL ATA before the route as well.
pub struct DynamicBuyRoute {
    pub instruction: Instruction,
    pub quote_mint: Pubkey,
    pub quote_token_program: Pubkey,
    pub quote_ata: Pubkey,
    pub output_mint: Pubkey,
    pub output_token_program: Pubkey,
    pub output_ata: Pubkey,
}

/// Build WSOL -> quote -> target-mint using explicit, already-resolved pools.
///
/// `first_leg_amount_in` is the first DEX's exact input. With a zero-fee Router
/// config it equals `route_amount_in`; otherwise it must account for the fee
/// taken from the WSOL ATA before the first CPI. The caller supplies a fresh
/// quote floor and a final output floor, plus all ATA/WSOL setup instructions.
/// No RPC call, simulation, signing, or broadcasting occurs here.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamic_quote_buy(
    program_id: &Pubkey,
    payer: &Pubkey,
    fee_recipient: &Pubkey,
    route_amount_in: u64,
    first_leg_amount_in: u64,
    quote_min_out: u64,
    min_target_out: u64,
    quote_pool: &Market,
    target_pool: &Market,
) -> Result<DynamicBuyRoute> {
    if first_leg_amount_in == 0 || first_leg_amount_in > route_amount_in {
        bail!("first leg amount must be within the Route input budget");
    }
    let (first, quote_mint, quote_program) =
        wsol_to_quote_leg(payer, quote_pool, first_leg_amount_in, quote_min_out)?;
    let quote_ata = ata(payer, &quote_mint, &quote_program);
    let (second, output_mint, output_program) = quote_to_target_leg(
        payer,
        target_pool,
        quote_mint,
        quote_program,
        min_target_out,
    )?;
    let output_ata = ata(payer, &output_mint, &output_program);
    let wsol_ata = ata(payer, &WSOL_MINT, &TOKEN_PROGRAM);
    let fee_ata = ata(fee_recipient, &WSOL_MINT, &TOKEN_PROGRAM);
    let instruction = build_dynamic_route_instruction(
        program_id,
        RouteAccounts {
            payer: *payer,
            fee_destination: fee_ata,
            fee_source: wsol_ata,
            output_token_account: output_ata,
            fee_program: TOKEN_PROGRAM,
            fee_mint: WSOL_MINT,
        },
        quote_ata,
        route_amount_in,
        quote_min_out,
        min_target_out,
        &output_mint,
        &first,
        &second,
    )?;
    Ok(DynamicBuyRoute {
        instruction,
        quote_mint,
        quote_token_program: quote_program,
        quote_ata,
        output_mint,
        output_token_program: output_program,
        output_ata,
    })
}

/// Convenience for the deployed program ID used by this SDK.
#[allow(clippy::too_many_arguments)]
pub fn build_deployed_dynamic_quote_buy(
    payer: &Pubkey,
    fee_recipient: &Pubkey,
    route_amount_in: u64,
    first_leg_amount_in: u64,
    quote_min_out: u64,
    min_target_out: u64,
    quote_pool: &Market,
    target_pool: &Market,
) -> Result<DynamicBuyRoute> {
    build_dynamic_quote_buy(
        &PROGRAM_ID,
        payer,
        fee_recipient,
        route_amount_in,
        first_leg_amount_in,
        quote_min_out,
        min_target_out,
        quote_pool,
        target_pool,
    )
}

fn wsol_to_quote_leg(
    user: &Pubkey,
    pool: &Market,
    amount_in: u64,
    min_out: u64,
) -> Result<(Leg, Pubkey, Pubkey)> {
    let (quote_mint, quote_program, leg) = match pool {
        Market::CpmmOuter(p) => {
            let quote = p
                .other_mint(&WSOL_MINT)
                .ok_or_else(|| anyhow!("CPMM bridge lacks WSOL"))?;
            if p.token_program_for(&WSOL_MINT) != Some(TOKEN_PROGRAM) {
                bail!("WSOL must use the classic Token Program");
            }
            let program = p
                .token_program_for(&quote)
                .ok_or_else(|| anyhow!("CPMM quote program missing"))?;
            let leg = cpmm_swap_leg(
                user,
                p,
                amount_in,
                min_out,
                WSOL_MINT,
                quote,
                ata(user, &WSOL_MINT, &TOKEN_PROGRAM),
                ata(user, &quote, &program),
            )?;
            (quote, program, leg)
        }
        Market::RaydiumAmmV4(p) => {
            if p.token_program != TOKEN_PROGRAM {
                bail!("AMM V4 WSOL must use the classic Token Program");
            }
            let quote = other(WSOL_MINT, p.coin_mint, p.pc_mint)?;
            (
                quote,
                p.token_program,
                raydium_amm_v4_swap_leg(user, p, amount_in, min_out, WSOL_MINT)?,
            )
        }
        Market::RaydiumClmm(p) => {
            let quote = other(WSOL_MINT, p.token_0_mint, p.token_1_mint)?;
            let source_program = if p.token_0_mint == WSOL_MINT {
                p.token_0_program
            } else {
                p.token_1_program
            };
            if source_program != TOKEN_PROGRAM {
                bail!("CLMM WSOL must use the classic Token Program");
            }
            let program = if p.token_0_mint == quote {
                p.token_0_program
            } else {
                p.token_1_program
            };
            (
                quote,
                program,
                raydium_clmm_swap_leg(user, p, amount_in, min_out, WSOL_MINT)?,
            )
        }
        Market::Whirlpool(p) => {
            let quote = other(WSOL_MINT, p.mint_a, p.mint_b)?;
            let source_program = if p.mint_a == WSOL_MINT {
                p.token_program_a
            } else {
                p.token_program_b
            };
            if source_program != TOKEN_PROGRAM {
                bail!("Whirlpool WSOL must use the classic Token Program");
            }
            let program = if p.mint_a == quote {
                p.token_program_a
            } else {
                p.token_program_b
            };
            (
                quote,
                program,
                whirlpool_swap_leg(user, p, amount_in, min_out, WSOL_MINT)?,
            )
        }
        Market::MeteoraDlmm(p) => {
            let quote = other(WSOL_MINT, p.token_x_mint, p.token_y_mint)?;
            let source_program = if p.token_x_mint == WSOL_MINT {
                p.token_x_program
            } else {
                p.token_y_program
            };
            if source_program != TOKEN_PROGRAM {
                bail!("DLMM WSOL must use the classic Token Program");
            }
            let program = if p.token_x_mint == quote {
                p.token_x_program
            } else {
                p.token_y_program
            };
            (
                quote,
                program,
                meteora_dlmm_swap_leg(user, p, amount_in, min_out, WSOL_MINT)?,
            )
        }
        Market::MeteoraDammV2(p) => {
            if p.swap_mode != 0 {
                bail!("DAMM v2 bridge must use exact-in mode");
            }
            let quote = other(WSOL_MINT, p.token_a_mint, p.token_b_mint)?;
            let source_program = if p.token_a_mint == WSOL_MINT {
                p.token_a_program
            } else {
                p.token_b_program
            };
            if source_program != TOKEN_PROGRAM {
                bail!("DAMM v2 WSOL must use the classic Token Program");
            }
            let program = if p.token_a_mint == quote {
                p.token_a_program
            } else {
                p.token_b_program
            };
            (
                quote,
                program,
                meteora_damm_v2_swap_leg(user, p, amount_in, min_out, WSOL_MINT)?,
            )
        }
        Market::PumpSwapOuter(p) => {
            if p.quote_mint == WSOL_MINT && p.quote_token_program == TOKEN_PROGRAM {
                (
                    p.base_mint,
                    p.base_token_program,
                    pumpswap_buy_leg(user, p, amount_in, min_out)?,
                )
            } else if p.base_mint == WSOL_MINT && p.base_token_program == TOKEN_PROGRAM {
                (
                    p.quote_mint,
                    p.quote_token_program,
                    pumpswap_sell_leg(user, p, amount_in, min_out)?,
                )
            } else {
                bail!("PumpSwap bridge lacks classic-program WSOL");
            }
        }
        _ => bail!("unsupported first-hop quote pool"),
    };
    Ok((leg, quote_mint, quote_program))
}

fn quote_to_target_leg(
    user: &Pubkey,
    pool: &Market,
    quote_mint: Pubkey,
    quote_program: Pubkey,
    min_out: u64,
) -> Result<(Leg, Pubkey, Pubkey)> {
    match pool {
        Market::LaunchLabInner(p) => {
            require_mint_programs(&[(p.base_mint, p.base_token_program),
                (p.quote_mint, p.quote_token_program)], "LaunchLab")?;
            if p.quote_mint != quote_mint || p.quote_token_program != quote_program {
                bail!("LaunchLab quote mint/program does not match first hop");
            }
            let output_ata = ata(user, &p.base_mint, &p.base_token_program);
            let quote_ata = ata(user, &quote_mint, &quote_program);
            Ok((
                launchlab_buy_leg(user, p, 1, min_out, output_ata, quote_ata),
                p.base_mint,
                p.base_token_program,
            ))
        }
        Market::CpmmOuter(p) => {
            let output_mint = p
                .other_mint(&quote_mint)
                .ok_or_else(|| anyhow!("target CPMM pool lacks the first hop quote mint"))?;
            if p.token_program_for(&quote_mint) != Some(quote_program) {
                bail!("target CPMM quote token program does not match first hop");
            }
            let output_program = p
                .token_program_for(&output_mint)
                .ok_or_else(|| anyhow!("target CPMM output token program missing"))?;
            let leg = cpmm_swap_leg(
                user,
                p,
                1,
                min_out,
                quote_mint,
                output_mint,
                ata(user, &quote_mint, &quote_program),
                ata(user, &output_mint, &output_program),
            )?;
            Ok((leg, output_mint, output_program))
        }
        _ => bail!("second hop must be LaunchLab or Raydium CPMM"),
    }
}

#[inline]
fn other(input: Pubkey, a: Pubkey, b: Pubkey) -> Result<Pubkey> {
    if input == a && a != b {
        Ok(b)
    } else if input == b && a != b {
        Ok(a)
    } else {
        bail!("first-hop pool does not pair WSOL with another mint")
    }
}
