#!/usr/bin/env python3
"""Run captured Pump/router ELF locally. Requires solders==0.29.0.

Uses ephemeral wallets, synthetic curves and a public mainnet fee snapshot.
Never calls RPC or submits network transactions. Boundary assertions describe
this captured deployment; re-evaluate them after a Pump program upgrade.
"""
import argparse
import base64
import hashlib
import json
import runpy
import struct
from pathlib import Path

from solders.account import Account
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.litesvm import LiteSVM
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from solders.transaction_metadata import FailedTransactionMetadata

helpers = runpy.run_path(str(Path(__file__).with_name("test-router-security.py")))
mint_account = helpers["mint_account"]
token_account = helpers["token_account"]
PUMP = Pubkey.from_string("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
ROUTER = Pubkey.from_string("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8")
TOKEN = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
TOKEN_2022 = Pubkey.from_string("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
ASSOCIATED = Pubkey.from_string("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
WSOL = Pubkey.from_string("So11111111111111111111111111111111111111112")
SYSTEM = Pubkey.default()
FEES = Pubkey.from_string("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ")
BUY_IN = bytes([225, 247, 80, 30, 213, 179, 132, 136])
BUY_OUT = bytes([7, 5, 29, 196, 245, 23, 101, 80])
SELL = bytes([28, 146, 222, 119, 38, 196, 105, 213])
BUDGET = 10_000_000
ROUTER_FEE = 100_000


def pda(seeds, program=PUMP):
    return Pubkey.find_program_address(seeds, program)[0]


def ata(owner, mint, program=TOKEN):
    return pda([bytes(owner), bytes(program), bytes(mint)], ASSOCIATED)


def bps_fee(amount, rate):
    return (amount * rate + 9999) // 10000


def account_with_program(account, program):
    return Account(account.lamports, account.data, program)


def send(vm, payer, instruction):
    return vm.send_transaction(Transaction.new_signed_with_payer(
        [instruction], payer.pubkey(), [payer], vm.latest_blockhash()))


def run_case(router_code, pump_code, *, boundary=False, token2022=False,
             initial_quote=0, exact_out=None, quote_input=9_900_000,
             expected_failure=False, sell_after=True, fee_fixture=None):
    vm = LiteSVM().with_default_programs()
    vm.add_program(PUMP, pump_code)
    vm.add_program(ROUTER, router_code)
    payer = Keypair()
    user = payer.pubkey()
    vm.airdrop(user, 10_000_000_000)
    mint = Pubkey.new_unique()
    base_program = TOKEN_2022 if token2022 else TOKEN
    curve = pda([b"bonding-curve", bytes(mint)])
    global_key = pda([b"global"])
    event = pda([b"__event_authority"])
    config = pda([b"fee_config", bytes(PUMP)], FEES)
    fixture_path = fee_fixture or Path(__file__).parent / "fixtures/pump-v3-mainnet-fees.json"
    fixture = json.loads(fixture_path.read_text())
    accounts = fixture["result"]["value"]
    for key, account in zip([global_key, config], accounts):
        vm.set_account(key, Account(account["lamports"], base64.b64decode(account["data"][0]),
                                    Pubkey.from_string(account["owner"])))
    global_data = base64.b64decode(accounts[0]["data"][0])
    fee_data = base64.b64decode(accounts[1]["data"][0])
    assert struct.unpack_from("<I", fee_data, 65)[0] == 1  # This fixture has one SOL tier.
    _, protocol_bps, creator_bps = struct.unpack_from("<QQQ", fee_data, 85)
    buyback = Pubkey.from_bytes(global_data[741:773])
    vm.airdrop(buyback, 2_000_000)
    base_mint = mint_account(user, 1_000_000_000_000_000)
    base_data = bytearray(base_mint.data)
    base_data[44] = 6
    vm.set_account(mint, Account(base_mint.lamports, bytes(base_data), base_program))
    quote_mint = mint_account(user, 1_000_000_000_000)
    quote_data = bytearray(quote_mint.data)
    quote_data[44] = 9
    vm.set_account(WSOL, Account(quote_mint.lamports, bytes(quote_data), TOKEN))
    remaining = 1_000_000_000 if boundary else 793_100_000_000_000
    virtual_base = 279_900_000_000_000 + remaining if boundary else 1_073_000_000_000_000
    virtual_quote = 1_073_000_000_000_000 * 30_000_000_000 // virtual_base if boundary else 30_000_000_000
    real_quote = virtual_quote - 30_000_000_000
    vault_balance = 206_900_000_000_000 + remaining
    data = bytearray(166)
    data[:8] = bytes([23, 183, 248, 55, 96, 216, 172, 96])
    for offset, value in [(8, virtual_base), (16, virtual_quote), (24, remaining),
                          (32, real_quote), (40, 1_000_000_000_000_000), (142, initial_quote)]:
        struct.pack_into("<Q", data, offset, value)
    data[49:81] = bytes(user)
    vm.set_account(curve, Account(real_quote + 3_000_000, bytes(data), PUMP))
    curve_base = ata(curve, mint, base_program)
    user_base = ata(user, mint, base_program)
    vm.set_account(curve_base, account_with_program(token_account(curve, mint, vault_balance), base_program))
    vm.set_account(user_base, account_with_program(token_account(user, mint, 0), base_program))
    volume = pda([b"user_volume_accumulator", bytes(user)])
    init = Instruction(PUMP, bytes([94, 6, 202, 115, 255, 96, 232, 183]), [
        AccountMeta(user, True, True), AccountMeta(user, False, False),
        AccountMeta(volume, False, True), AccountMeta(SYSTEM, False, False),
        AccountMeta(event, False, False), AccountMeta(PUMP, False, False)])
    result = send(vm, payer, init)
    assert not isinstance(result, FailedTransactionMetadata), (result.err(), result.meta().logs()) if isinstance(result, FailedTransactionMetadata) else None
    keys = [global_key, mint, WSOL, base_program, TOKEN, curve, curve_base,
            ata(curve, WSOL), user, user_base, ata(user, WSOL), volume, config, buyback,
            SYSTEM, event, PUMP]
    metas = [AccountMeta(key, i == 8, i in [5, 6, 7, 8, 9, 10, 11, 13]) for i, key in enumerate(keys)]
    router_config, bump = Pubkey.find_program_address([b"config"], ROUTER)
    recipient = Pubkey.new_unique()
    vm.airdrop(recipient, 2_000_000)
    vm.set_account(router_config, Account(2_000_000, b"ROUTCFG1" + bytes(user) + bytes(recipient)
        + struct.pack("<HBB", 100, bump, 0) + bytes(4), ROUTER))

    def route(dex, amount, minimum, asset, output, source, fee_dest, fee_program, expected):
        payload = bytes([2]) + struct.pack("<QQBB", amount, minimum, asset, 1) + bytes(expected)
        payload += bytes(PUMP) + bytes([len(metas)]) + struct.pack("<H", len(dex)) + dex
        fixed = [AccountMeta(user, True, True), AccountMeta(router_config, False, False),
                 AccountMeta(fee_dest, False, True), AccountMeta(source, False, True),
                 AccountMeta(output, False, True), AccountMeta(fee_program, False, False)]
        return Instruction(ROUTER, payload, fixed + metas + [AccountMeta(PUMP, False, False)])

    dex = BUY_OUT + struct.pack("<QQB", exact_out, 9_900_000, 0) if exact_out is not None else BUY_IN + struct.pack("<QQB", quote_input, 1, 0)
    initial_sol = vm.get_account(user).lamports
    initial_fee = vm.get_account(recipient).lamports
    result = send(vm, payer, route(dex, BUDGET, exact_out or 1, 0x80, user_base, user, recipient, SYSTEM, mint))
    error = str(result.err()) if isinstance(result, FailedTransactionMetadata) else None
    tokens = struct.unpack_from("<Q", vm.get_account(user_base).data, 64)[0]
    observed = {"mode": "exact_out" if exact_out is not None else "quote_input",
                "amount": exact_out if exact_out is not None else quote_input,
                "token2022": token2022, "initial_quote": initial_quote, "error": error,
                "complete": vm.get_account(curve).data[48], "tokens": tokens}
    if expected_failure:
        assert error and "InstructionErrorCustom(6021)" in error, observed
        assert vm.get_account(recipient).lamports == initial_fee, observed
        assert vm.get_account(curve).data == bytes(data), observed
        assert struct.unpack_from("<Q", vm.get_account(curve_base).data, 64)[0] == vault_balance
        assert tokens == 0, observed
        return observed
    assert error is None, (observed, result.meta().logs()) if error else observed
    net = quote_input * 10000 // (10000 + protocol_bps + creator_bps)
    net -= max(0, net + bps_fee(net, protocol_bps) + bps_fee(net, creator_bps) - quote_input)
    expected = exact_out if exact_out is not None else (net - 1) * virtual_base // (virtual_quote + net - 1)
    if exact_out is None and expected > remaining:
        curve_net = remaining * virtual_quote // (virtual_base - remaining) + 1
        leftover = quote_input - curve_net - bps_fee(curve_net, protocol_bps) - bps_fee(curve_net, creator_bps)
        after_fees = leftover * 10000 // (10000 + protocol_bps + creator_bps)
        leg_net = after_fees - max(0, after_fees + bps_fee(after_fees, protocol_bps)
                                    + bps_fee(after_fees, creator_bps) - leftover)
        migration_fee = struct.unpack_from("<Q", global_data, 146)[0]
        pool_quote = real_quote + curve_net - migration_fee
        pool_base = vault_balance - remaining
        assert pool_quote > 0 and pool_base > 0
        expected = remaining if after_fees < 2 else remaining + (leg_net - 1) * pool_base // (pool_quote + leg_net - 1)
    assert tokens == expected, (observed, expected)
    assert observed["complete"] == int(tokens >= remaining), observed
    assert vm.get_account(recipient).lamports - initial_fee == ROUTER_FEE
    observed["spent"] = initial_sol - vm.get_account(user).lamports - 5000
    assert ROUTER_FEE <= observed["spent"] <= BUDGET, observed
    if exact_out is not None:
        curve_tokens = min(exact_out, remaining)
        curve_net = curve_tokens * virtual_quote // (virtual_base - curve_tokens) + 1
        expected_cost = curve_net + bps_fee(curve_net, protocol_bps) + bps_fee(curve_net, creator_bps)
        if exact_out > remaining:
            pool_base = vault_balance - remaining
            pool_quote = real_quote + curve_net - struct.unpack_from("<Q", global_data, 146)[0]
            past_curve = exact_out - remaining
            assert pool_base > past_curve and pool_quote > 0
            leg_net = (pool_quote * past_curve + pool_base - past_curve - 1) // (pool_base - past_curve)
            expected_cost += leg_net + bps_fee(leg_net, protocol_bps) + bps_fee(leg_net, creator_bps)
        assert observed["spent"] == ROUTER_FEE + expected_cost, (observed, expected_cost)
    if observed["complete"]:
        # Crossing is allowed within the completing buy, not on subsequent trades.
        before = {key: vm.get_account(key) for key in [user, recipient, curve, curve_base, user_base]}
        rejected = send(vm, payer, route(BUY_IN + struct.pack("<QQB", 1_000_000, 1, 0),
                                        BUDGET, 1, 0x80, user_base, user, recipient, SYSTEM, mint))
        assert isinstance(rejected, FailedTransactionMetadata), observed
        assert "InstructionErrorCustom(6005)" in str(rejected.err()), rejected.meta().logs()
        for key, account in before.items():
            after = vm.get_account(key)
            assert after.data == account.data and after.lamports == account.lamports - (5000 if key == user else 0)
        observed["completed_curve_buy_error"] = 6005
    if not sell_after:
        return observed
    fee_base = ata(recipient, mint, base_program)
    vm.set_account(fee_base, account_with_program(token_account(recipient, mint, 0), base_program))
    amount = tokens // 2
    fee = amount // 100
    initial_sol = vm.get_account(user).lamports
    result = send(vm, payer, route(SELL + struct.pack("<QQ", amount - fee, 1), amount, 1, 1,
                                  user, user_base, fee_base, base_program, SYSTEM))
    assert not isinstance(result, FailedTransactionMetadata), (result.err(), result.meta().logs()) if isinstance(result, FailedTransactionMetadata) else None
    assert struct.unpack_from("<Q", vm.get_account(user_base).data, 64)[0] == tokens - amount
    assert struct.unpack_from("<Q", vm.get_account(fee_base).data, 64)[0] == fee
    observed["sell_received"] = vm.get_account(user).lamports - initial_sol + 5000
    assert observed["sell_received"] > 0, observed
    return observed


def feature_probe(pump_code, graduation_supported):
    vm = LiteSVM().with_default_programs()
    vm.add_program(PUMP, pump_code)
    payer = Keypair()
    vm.airdrop(payer.pubkey(), 100_000_000)
    data = hashlib.sha256(b"global:set_max_curve_depth").digest()[:8] + bytes([1])
    result = send(vm, payer, Instruction(PUMP, data, []))
    assert isinstance(result, FailedTransactionMetadata)
    expected_error = 3005 if graduation_supported else 101
    assert f"InstructionErrorCustom({expected_error})" in str(result.err()), result.meta().logs()
    print(json.dumps({"feature_probe": "set_max_curve_depth", "error": expected_error}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("router", type=Path)
    parser.add_argument("pump", type=Path)
    parser.add_argument("--fee-fixture", type=Path,
                        help="Global/FeeConfig getMultipleAccounts snapshot matching the captured Pump ELF")
    parser.add_argument("--graduation-supported", action="store_true",
                        help="Expect crossing buys to succeed for the upgraded captured deployment")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--graduation-rejection", action="store_true")
    modes.add_argument("--exact-out", action="store_true")
    modes.add_argument("--boundary-matrix", action="store_true")
    args = parser.parse_args()
    if args.graduation_supported and args.graduation_rejection:
        parser.error("--graduation-supported conflicts with --graduation-rejection")
    router, pump = args.router.read_bytes(), args.pump.read_bytes()
    if args.boundary_matrix:
        feature_probe(pump, args.graduation_supported)
        # Official fee-inclusive quote for the remaining 1e9 raw base units.
        virtual_base = 279_901_000_000_000
        virtual_quote = 1_073_000_000_000_000 * 30_000_000_000 // virtual_base
        net = 1_000_000_000 * virtual_quote // (virtual_base - 1_000_000_000) + 1
        fixture_path = args.fee_fixture or Path(__file__).parent / "fixtures/pump-v3-mainnet-fees.json"
        fee_data = base64.b64decode(json.loads(fixture_path.read_text())["result"]["value"][1]["data"][0])
        assert struct.unpack_from("<I", fee_data, 65)[0] == 1
        _, protocol_bps, creator_bps = struct.unpack_from("<QQQ", fee_data, 85)
        cost = net + bps_fee(net, protocol_bps) + bps_fee(net, creator_bps)
        count = 0
        for token2022 in [False, True]:
            for initial_quote in [0, 30_000_000_000]:
                for amount in [999_999_999, 1_000_000_000, 1_000_000_001]:
                    print(json.dumps(run_case(router, pump, boundary=True, token2022=token2022,
                        initial_quote=initial_quote, exact_out=amount,
                        expected_failure=not args.graduation_supported and amount > 1_000_000_000, sell_after=False, fee_fixture=args.fee_fixture)))
                    count += 1
                for amount in [cost - 1, cost, cost + 1, cost + 100, 9_900_000]:
                    print(json.dumps(run_case(router, pump, boundary=True, token2022=token2022,
                        initial_quote=initial_quote, quote_input=amount,
                        expected_failure=not args.graduation_supported and amount > cost, sell_after=False, fee_fixture=args.fee_fixture)))
                    count += 1
        print(json.dumps({"boundary_cases_passed": count, "curve_cost": cost}))
    else:
        print(json.dumps(run_case(router, pump, boundary=args.graduation_rejection,
            expected_failure=args.graduation_rejection,
            exact_out=100_000_000_000 if args.exact_out else None, fee_fixture=args.fee_fixture)))
    print(json.dumps({"pump_sha256": hashlib.sha256(pump).hexdigest(),
                      "router_sha256": hashlib.sha256(router).hexdigest()}))


if __name__ == "__main__":
    main()
