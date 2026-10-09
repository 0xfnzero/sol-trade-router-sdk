#!/usr/bin/env python3
"""Execute Router SBF and a test-only CPMM-layout DEX in LiteSVM.

The fixture uses real SPL/Token-2022 CPIs and synthetic 2:1 outputs. It tests
Router accounting/rollback, not CPMM pricing or live DEX execution. No RPC,
real wallets, deployment or broadcast. Requires solders==0.29.0.
"""
import argparse
import hashlib
import json
import re
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
ROUTER = helpers["PROGRAM"]
CPMM = Pubkey.from_string("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C")
DISC = bytes([143, 190, 90, 218, 196, 30, 51, 222])


def run_case(router_code, dex_code, hops, initial, transfer_fee, fault=None):
    vm = LiteSVM().with_default_programs()
    vm.add_program(ROUTER, router_code)
    vm.add_program(CPMM, dex_code)
    payer = Keypair()
    user = payer.pubkey()
    vm.airdrop(user, 1_000_000_000)
    program = helpers["TOKEN_2022"] if transfer_fee else helpers["TOKEN"]
    recipient = Pubkey.new_unique()
    config, bump = Pubkey.find_program_address([b"config"], ROUTER)
    vm.set_account(config, Account(2_000_000, b"ROUTCFG1" + bytes(user) + bytes(recipient)
                                  + struct.pack("<HBB", 100, bump, 0) + bytes(4), ROUTER))
    mints = [Pubkey.new_unique() for _ in range(hops + 1)]
    wallets = [Pubkey.new_unique() for _ in mints]
    tracked = []

    def token(key, mint, amount, owner=user):
        vm.set_account(key, helpers["token_account"](owner, mint, amount, program, transfer_fee))
        tracked.append(key)

    for i, (mint, wallet) in enumerate(zip(mints, wallets)):
        vm.set_account(mint, helpers["mint_account"](user, 10_000_000, program, transfer_fee))
        token(wallet, mint, 20_000 if i == 0 else initial + i)
    fee_dest = Pubkey.new_unique()
    token(fee_dest, mints[0], 0, recipient)
    sinks = [Pubkey.new_unique() for _ in range(hops)]
    reserves = [Pubkey.new_unique() for _ in range(hops)]
    for i in range(hops):
        token(sinks[i], mints[i], 0)
        token(reserves[i], mints[i + 1], 1_000_000)
    before = {key: vm.get_account(key).data for key in tracked}
    intermediate = wallets[1:-1]
    expected_mint = mints[-1] if fault != "wrong_mint" else mints[0]
    metas = [AccountMeta(user, True, True), AccountMeta(config, False, False),
             AccountMeta(fee_dest, False, True), AccountMeta(wallets[0], False, True),
             AccountMeta(wallets[-1], False, True), AccountMeta(program, False, False)]
    metas += [AccountMeta(wallets[0] if fault == "alias" and i == 0 else key, False, True)
              for i, key in enumerate(intermediate)]

    def credited(amount):
        return amount - (amount * 100 + 9999) // 10000 if transfer_fee else amount

    amounts = [9_900]
    for _ in range(hops):
        amounts.append(credited(2 * amounts[-1]))
    minimum = amounts[-1] + (fault == "final_slippage")
    data = bytearray([5 if hops == 2 else 6]) + struct.pack("<QQBB", 10_000, minimum, 1, hops)
    data += bytes(expected_mint)
    for i in range(hops - 1):
        data += struct.pack("<Q", amounts[i + 1] + (fault == f"intermediate_slippage_{i}"))
    for i in range(hops):
        amount = 9_900 if i == 0 else 0
        if fault == "nonzero_placeholder" and i == hops - 1:
            amount = 1
        mode = 2 if fault == f"underspend_{i}" else 3 if fault == f"overspend_{i}" else 1
        leg_data = DISC + struct.pack("<QQ", amount, mode)
        leg_metas = [AccountMeta(user, True, False)] + [AccountMeta(Pubkey.default(), False, False)] * 3
        leg_metas += [AccountMeta(wallets[i], False, True), AccountMeta(wallets[i + 1], False, True),
                      AccountMeta(sinks[i], False, True), AccountMeta(reserves[i], False, True),
                      AccountMeta(program, False, False), AccountMeta(program, False, False),
                      AccountMeta(mints[i], False, False), AccountMeta(mints[i + 1], False, False),
                      AccountMeta(Pubkey.default(), False, False)]
        metas += leg_metas
        data += bytes(CPMM) + struct.pack("<BH", len(leg_metas), len(leg_data)) + leg_data
    metas.append(AccountMeta(CPMM, False, False))
    tx = Transaction.new_signed_with_payer([Instruction(ROUTER, bytes(data), metas)],
                                         user, [payer], vm.latest_blockhash())
    tx.verify()
    result = vm.send_transaction(tx)
    failed = isinstance(result, FailedTransactionMetadata)
    error = str(result.err()) if failed else ""
    match = re.search(r"InstructionErrorCustom\((\d+)\)", error)
    actual_error = int(match.group(1)) if match else None
    expected_error = None
    if fault:
        expected_error = (18 if fault == "wrong_mint" else 21 if fault == "alias"
                          else 19 if fault == "nonzero_placeholder" else
                          10 if "slippage" in fault else 20)
    assert failed == (expected_error is not None) and actual_error == expected_error, (fault, error)
    if failed:
        # Every CPI and the platform fee roll back, including withheld fees.
        assert all(vm.get_account(key).data == data for key, data in before.items()), (fault, error)
    else:
        balance = lambda key: helpers["balance"](vm, key)
        assert balance(wallets[0]) == 10_000
        assert balance(fee_dest) == credited(100)
        assert balance(wallets[-1]) == initial + hops + amounts[-1]
        for i, key in enumerate(intermediate, 1):
            assert balance(key) == initial + i, "pre-existing intermediate balance was spent"
        metadata = result
        assert sum(log.startswith(f"Program {CPMM} invoke") for log in metadata.logs()) == hops
    return {"hops": hops, "initial": initial, "token_2022_transfer_fee": transfer_fee,
            "fault": fault, "success": not failed, "custom_error": actual_error}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("router", type=Path)
    parser.add_argument("dex", type=Path)
    args = parser.parse_args()
    router_code, dex_code = args.router.read_bytes(), args.dex.read_bytes()
    cases = 0
    for hops in [2, 3]:
        for initial in [0, 777]:
            for transfer_fee in [False, True]:
                print(json.dumps(run_case(router_code, dex_code, hops, initial, transfer_fee)))
                cases += 1
        faults = ["wrong_mint", "alias", "final_slippage", "intermediate_slippage_0"]
        for i in range(1, hops):
            faults += [f"underspend_{i}", f"overspend_{i}"]
        if hops == 3:
            faults += ["intermediate_slippage_1", "nonzero_placeholder"]
        for fault in faults:
            for transfer_fee in [False, True]:
                print(json.dumps(run_case(router_code, dex_code, hops, 777, transfer_fee, fault)))
                cases += 1
    print(json.dumps({"passed": cases, "router_sha256": hashlib.sha256(router_code).hexdigest(),
                      "mock_dex_sha256": hashlib.sha256(dex_code).hexdigest()}))


if __name__ == "__main__":
    main()
