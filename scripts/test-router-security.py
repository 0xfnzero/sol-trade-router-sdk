#!/usr/bin/env python3
"""Execute the actual router ELF in LiteSVM, using only ephemeral local accounts.

Requires solders==0.29.0. No RPC calls, real keys, or network transactions.
Pass --pr1-format to reproduce the vulnerable PR #1 protocol instead of the fix.
"""

import argparse
import hashlib
import json
import re
import struct
from pathlib import Path

from solders.account import Account
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.litesvm import LiteSVM
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from solders.transaction_metadata import FailedTransactionMetadata

PROGRAM = Pubkey.from_string("CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8")
TOKEN = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
TOKEN_2022 = Pubkey.from_string("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
SYSTEM = Pubkey.default()


def token_account(user, mint, balance, program=TOKEN, transfer_fee=False):
    data = bytearray(165)
    data[:32] = bytes(mint)
    data[32:64] = bytes(user)
    data[64:72] = struct.pack("<Q", balance)
    data[108] = 1  # SPL Token AccountState::Initialized.
    if transfer_fee:
        data += bytes([2]) + struct.pack("<HHQ", 2, 8, 0)  # TransferFeeAmount TLV.
    return Account(3_000_000, bytes(data), program)


def balance(vm, address):
    return struct.unpack("<Q", vm.get_account(address).data[64:72])[0]


def mint_account(authority, supply, program=TOKEN, transfer_fee=False, decimals=0):
    data = (struct.pack("<I", 1) + bytes(authority) + struct.pack("<QBB", supply, decimals, 1)
            + struct.pack("<I", 0) + bytes(32))
    if transfer_fee:
        # TransferFeeConfig: no authorities/withheld fees; both epochs charge 1%.
        schedule = struct.pack("<QQH", 0, (1 << 64) - 1, 100)
        config = bytes(72) + schedule + schedule
        assert len(config) == 108
        data += bytes(165 - len(data)) + bytes([1]) + struct.pack("<HH", 1, 108) + config
    return Account(3_000_000, data, program)


def run_case(code, name, declared, swap_input, *, bound_mint, wrong_mint=False,
             native_output=False, minimum=1, input_balance=20_000,
             token_2022=False, transfer_fee=False, fee_mint_mode="valid", fee_only=False):
    vm = LiteSVM().with_default_programs()
    vm.add_program(PROGRAM, code)
    payer = Keypair()
    user = payer.pubkey()
    vm.airdrop(user, 1_000_000_000)
    config_address, bump = Pubkey.find_program_address([b"config"], PROGRAM)
    # A reachable initialized config with a nonzero fee, independent of mainnet's
    # current fee setting. Sources belong to the signer; fees go to a separate user.
    recipient = Pubkey.new_unique()
    config = (b"ROUTCFG1" + bytes(user) + bytes(recipient)
              + struct.pack("<HBB", 100, bump, 0) + bytes(4))
    vm.set_account(config_address, Account(2_000_000, config, PROGRAM))
    source, sink, out_source, output, fee_dest = [Pubkey.new_unique() for _ in range(5)]
    mint_in, mint_out = Pubkey.new_unique(), Pubkey.new_unique()
    fee_program = TOKEN_2022 if token_2022 else TOKEN
    decimals = 9 if token_2022 else 0
    vm.set_account(mint_in, mint_account(user, input_balance, fee_program, transfer_fee, decimals))
    vm.set_account(mint_out, mint_account(user, 100))
    for address, mint, amount in [(source, mint_in, input_balance), (sink, mint_in, 0),
                                 (out_source, mint_out, 100), (output, mint_out, 0),
                                 (fee_dest, mint_in, 0)]:
        owner = recipient if address == fee_dest else user
        program = fee_program if mint == mint_in else TOKEN
        vm.set_account(address, token_account(owner, mint, amount, program,
                                             transfer_fee and mint == mint_in))
    expected_mint = Pubkey.new_unique() if wrong_mint else mint_out
    if wrong_mint:
        vm.set_account(expected_mint, mint_account(user, 0))
    signers = [payer]
    if native_output:
        expected_mint, output = SYSTEM, user
        funding = Keypair()
        vm.airdrop(funding.pubkey(), 1_000_000_000)
        signers.append(funding)
    metas = [AccountMeta(user, True, True), AccountMeta(config_address, False, False),
             AccountMeta(fee_dest, False, True), AccountMeta(source, False, True),
             AccountMeta(output, False, True), AccountMeta(fee_program, False, False)]
    data = bytearray([2]) + struct.pack("<QQBB", declared, minimum,
                                      0x81 if fee_only else 1, 1 if fee_only else 2)
    if bound_mint:
        data += bytes(expected_mint)
    trailing_metas = []

    def leg(program, accounts, instruction_data):
        nonlocal data
        data += bytes(program) + bytes([len(accounts)])
        data += struct.pack("<H", len(instruction_data)) + instruction_data
        metas.extend(accounts)

    # Real SPL Token and System Program CPIs isolate the router's checks from
    # DEX pricing/liquidity. This is not a simulation of a live PumpFun pool.
    if fee_only:
        trailing_metas.append(AccountMeta(mint_in, False, False))
    elif token_2022 and fee_mint_mode == "missing":
        leg(fee_program, [AccountMeta(source, False, True), AccountMeta(sink, False, True),
                         AccountMeta(user, True, False)],
            bytes([3]) + struct.pack("<Q", swap_input))
        trailing_metas.append(AccountMeta(fee_program, False, False))
    elif token_2022:
        leg(fee_program, [AccountMeta(source, False, True), AccountMeta(mint_in, False, False),
                         AccountMeta(sink, False, True), AccountMeta(user, True, False)],
            bytes([12]) + struct.pack("<QB", swap_input, decimals))
        trailing_metas.append(AccountMeta(fee_program, False, False))
        if fee_mint_mode == "invalid_owner":
            vm.set_account(mint_in, mint_account(user, input_balance))
        elif fee_mint_mode == "invalid_data":
            vm.set_account(mint_in, Account(3_000_000, bytes(81), TOKEN_2022))
    else:
        leg(TOKEN, [AccountMeta(source, False, True), AccountMeta(sink, False, True),
                    AccountMeta(user, True, False)], bytes([3]) + struct.pack("<Q", swap_input))
    if native_output:
        leg(SYSTEM, [AccountMeta(funding.pubkey(), True, True), AccountMeta(user, False, True)],
            struct.pack("<IQ", 2, 500))  # SystemInstruction::Transfer.
        metas.append(AccountMeta(SYSTEM, False, False))
    else:
        leg(TOKEN, [AccountMeta(out_source, False, True), AccountMeta(output, False, True),
                    AccountMeta(user, True, False)], bytes([3]) + struct.pack("<Q", 1))
    metas.append(AccountMeta(TOKEN, False, False))
    metas.extend(trailing_metas)
    tx = Transaction.new_signed_with_payer(
        [Instruction(PROGRAM, bytes(data), metas)], user, signers, vm.latest_blockhash())
    tx.verify()
    result = vm.send_transaction(tx)  # LiteSVM only; never broadcasts to Solana.
    failed = isinstance(result, FailedTransactionMetadata)
    error = str(result.err()) if failed else None
    match = re.search(r"InstructionErrorCustom\((\d+)\)", error or "")
    metadata = result.meta() if failed else result
    return {"case": name, "success": not failed,
            "custom_error": int(match.group(1)) if match else None, "error": error,
            "input_spent": input_balance - balance(vm, source), "fee_paid": balance(vm, fee_dest),
            "swap_received": balance(vm, sink),
            "fee_withheld": (struct.unpack("<Q", vm.get_account(fee_dest).data[170:178])[0]
                             if transfer_fee else 0),
            "output_received": None if native_output else balance(vm, output),
            "logs": metadata.logs()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("program", type=Path, help="Router SBF .so file")
    parser.add_argument("--pr1-format", action="store_true", help="Assert the known PR #1 regressions")
    args = parser.parse_args()
    code = args.program.read_bytes()
    old = args.pr1_format
    cases = [
        ("exact_input", 10_000, 9_900, {}, None),
        ("understated_fee", 1, 10_000, {}, None if old else 16),
        ("overspent_budget", 100, 10_000, {}, None if old else 16),
        ("wrong_output_mint", 1, 1, {"wrong_mint": True}, None if old else 18),
        ("native_sol_settlement", 1, 1, {"native_output": True, "minimum": 500}, 17 if old else None),
        ("native_sol_slippage", 1, 1, {"native_output": True, "minimum": 501}, 17 if old else 10),
    ]
    if not old:
        maximum = (1 << 64) - 1
        fee = maximum * 100 // 10_000
        cases.extend([
            ("large_exact_input", maximum, maximum - fee, {"input_balance": maximum}, None),
            ("clamped_exact_input", 101, 100, {}, None),
            ("token2022_transfer_fee", 10_000, 9_900,
             {"token_2022": True, "transfer_fee": True}, None),
            ("token2022_plain", 10_000, 9_900, {"token_2022": True}, None),
            ("token2022_wrong_mint_owner", 10_000, 9_900,
             {"token_2022": True, "fee_mint_mode": "invalid_owner"}, 9),
            ("token2022_truncated_mint", 10_000, 9_900,
             {"token_2022": True, "fee_mint_mode": "invalid_data"}, 9),
            ("token2022_legacy_plain", 10_000, 9_900,
             {"token_2022": True, "fee_mint_mode": "missing"}, None),
            ("token2022_missing_fee_mint", 10_000, 9_900,
             {"token_2022": True, "transfer_fee": True, "fee_mint_mode": "missing"}, 31),
            ("token2022_trailing_fee_mint", 10_000, 0,
             {"token_2022": True, "transfer_fee": True, "fee_only": True}, None),
        ])
    for name, declared, swap_input, options, expected_error in cases:
        result = run_case(code, name, declared, swap_input, bound_mint=not old, **options)
        assert result["success"] == (expected_error is None), result
        assert result["custom_error"] == expected_error, result
        if expected_error is not None:
            assert result["input_spent"] == result["fee_paid"] == 0, result  # Atomic rollback.
            assert result["swap_received"] == result["fee_withheld"] == 0, result
        elif name == "exact_input":
            assert result["input_spent"] == 10_000 and result["fee_paid"] == 100, result
        elif name == "understated_fee":
            assert result["input_spent"] == 10_000 and result["fee_paid"] == 0, result
        elif name in ("large_exact_input", "clamped_exact_input"):
            assert result["input_spent"] == declared, result
            assert result["fee_paid"] == declared * 100 // 10_000, result
        elif name == "token2022_transfer_fee":
            assert result["input_spent"] == 10_000 and result["fee_paid"] == 99, result
            assert result["fee_withheld"] == 1 and result["swap_received"] == 9_801, result
        elif name in ("token2022_plain", "token2022_legacy_plain"):
            assert result["input_spent"] == 10_000 and result["fee_paid"] == 100, result
        elif name == "token2022_trailing_fee_mint":
            assert result["input_spent"] == 100 and result["fee_paid"] == 99, result
            assert result["fee_withheld"] == 1 and result["swap_received"] == 0, result
        print(json.dumps({k: v for k, v in result.items() if k != "logs"}))
    if old:
        result = run_case(code, "pre_pr_tag2_header", 1, 1, bound_mint=True)
        assert not result["success"] and result["custom_error"] == 12, result
        assert result["input_spent"] == result["fee_paid"] == 0, result
        print(json.dumps({k: v for k, v in result.items() if k != "logs"}))
    print("PASS: " + hashlib.sha256(code).hexdigest())


if __name__ == "__main__":
    main()
