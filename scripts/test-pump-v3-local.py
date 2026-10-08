#!/usr/bin/env python3
"""Local-only Pump V3/router integration with captured ELF and public fee state.

Requires solders==0.29.0. Never uses RPC, real wallets, or network broadcasts.
The captured mainnet Pump ELF currently rejects the graduation fixture (6021).
"""
import sys, json, base64, struct, argparse
from pathlib import Path
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('router', type=Path)
parser.add_argument('pump', type=Path)
parser.add_argument('--graduation-rejection', action='store_true')
parser.add_argument('--exact-out', action='store_true')
args = parser.parse_args()
graduation = args.graduation_rejection
exact_out = args.exact_out
if graduation and exact_out:
    parser.error('run graduation and exact-output cases separately')
import runpy
h = runpy.run_path(str(Path(__file__).with_name('test-router-security.py')))
(token_account, mint_account) = (h['token_account'], h['mint_account'])
from solders.account import Account
from solders.instruction import Instruction, AccountMeta
from solders.keypair import Keypair
from solders.litesvm import LiteSVM
from solders.pubkey import Pubkey
from solders.transaction import Transaction
from solders.transaction_metadata import FailedTransactionMetadata
P = Pubkey.from_string('6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P')
T = Pubkey.from_string('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
A = Pubkey.from_string('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL')
Q = Pubkey.from_string('So11111111111111111111111111111111111111112')
S = Pubkey.default()

def pda(seeds, p=P):
    return Pubkey.find_program_address(seeds, p)[0]

def ata(u, m):
    return pda([bytes(u), bytes(T), bytes(m)], A)
vm = LiteSVM().with_default_programs()
vm.add_program(P, args.pump.read_bytes())
user = Keypair()
u = user.pubkey()
vm.airdrop(u, 10000000000)
m = Pubkey.new_unique()
c = pda([b'bonding-curve', bytes(m)])
g = pda([b'global'])
e = pda([b'__event_authority'])
f = Pubkey.find_program_address([b'fee_config', bytes(P)], Pubkey.from_string('pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ'))[0]
fixture = json.load(open(Path(__file__).parent / 'fixtures/pump-v3-mainnet-fees.json'))['result']['value']
for (k, a) in zip([g, f], fixture):
    vm.set_account(k, Account(a['lamports'], base64.b64decode(a['data'][0]), Pubkey.from_string(a['owner'])))
global_data = base64.b64decode(fixture[0]['data'][0])
buyback = Pubkey.from_bytes(global_data[741:773])
vm.airdrop(buyback, 2000000)
vm.set_account(m, mint_account(u, 1000000000000000))
vm.set_account(Q, mint_account(u, 1000000000000))
d = bytearray(166)
d[:8] = bytes([23, 183, 248, 55, 96, 216, 172, 96])
for (o, v) in [(8, 1073000000000000), (16, 30000000000), (24, 100000000000 if graduation else 793100000000000), (32, 20000000000 if graduation else 0), (40, 1000000000000000)]:
    d[o:o + 8] = struct.pack('<Q', v)
if graduation:
    remaining = 1000000000
    vb = 279900000000000 + remaining
    vq = 1073000000000000 * 30000000000 // vb
    rq = vq - 30000000000
    for (o, value) in [(8, vb), (16, vq), (24, remaining), (32, rq)]:
        d[o:o + 8] = struct.pack('<Q', value)
d[49:81] = bytes(u)
vm.set_account(c, Account(rq + 3000000 if graduation else 3000000, bytes(d), P))
vm.set_account(ata(c, m), token_account(c, m, 206900000000000 + remaining if graduation else 1000000000000000))
vm.set_account(ata(u, m), token_account(u, m, 0))
v = pda([b'user_volume_accumulator', bytes(u)])
keys = [(u, True, True), (u, False, False), (v, False, True), (S, False, False), (e, False, False), (P, False, False)]
ix = Instruction(P, bytes([94, 6, 202, 115, 255, 96, 232, 183]), [AccountMeta(*a) for a in keys])
r = vm.send_transaction(Transaction.new_signed_with_payer([ix], u, [user], vm.latest_blockhash()))
if isinstance(r, FailedTransactionMetadata):
    print('INIT', r.err(), r.meta().logs())
    sys.exit(1)
keys = [g, m, Q, T, T, c, ata(c, m), ata(c, Q), u, ata(u, m), ata(u, Q), v, f, buyback, S, e, P]
metas = [AccountMeta(k, i == 8, i in [5, 6, 7, 8, 9, 10, 11, 13]) for (i, k) in enumerate(keys)]
R = Pubkey.from_string('CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8')
vm.add_program(R, args.router.read_bytes())
(config, bump) = Pubkey.find_program_address([b'config'], R)
recipient = Pubkey.new_unique()
vm.airdrop(recipient, 2000000)
vm.set_account(config, Account(2000000, b'ROUTCFG1' + bytes(u) + bytes(recipient) + struct.pack('<HBB', 100, bump, 0) + bytes(4), R))

def route(dex, amount, minimum, asset, output, source, fee_dest, fee_program, expected):
    data = bytes([2]) + struct.pack('<QQBB', amount, minimum, asset, 1) + bytes(expected)
    data += bytes(P) + bytes([len(metas)]) + struct.pack('<H', len(dex)) + dex
    fixed = [AccountMeta(u, True, True), AccountMeta(config, False, False), AccountMeta(fee_dest, False, True), AccountMeta(source, False, True), AccountMeta(output, False, True), AccountMeta(fee_program, False, False)]
    return Instruction(R, data, fixed + metas + [AccountMeta(P, False, False)])
dex = bytes([7, 5, 29, 196, 245, 23, 101, 80]) + struct.pack('<QQB', 100000000000, 9900000, 0) if exact_out else bytes([225, 247, 80, 30, 213, 179, 132, 136]) + struct.pack('<QQB', 9900000, 1, 0)
ix = route(dex, 10000000, 100000000000 if exact_out else 1, 128, ata(u, m), u, recipient, S, m)
initial_sol = vm.get_account(u).lamports
initial_fee = vm.get_account(recipient).lamports
r = vm.send_transaction(Transaction.new_signed_with_payer([ix], u, [user], vm.latest_blockhash()))
if graduation:
    assert isinstance(r, FailedTransactionMetadata), 'graduation fixture unexpectedly succeeded; recheck deployment capability'
    assert 'InstructionErrorCustom(6021)' in str(r.err()), (r.err(), r.meta().logs())
    assert vm.get_account(recipient).lamports == initial_fee
    assert struct.unpack('<Q', vm.get_account(ata(u, m)).data[64:72])[0] == 0
    print('PASS: captured Pump ELF rejects graduation with 6021; route fee/output roll back')
    sys.exit(0)
if isinstance(r, FailedTransactionMetadata):
    print('BUY', r.err(), r.meta().logs())
    sys.exit(1)
tokens = struct.unpack('<Q', vm.get_account(ata(u, m)).data[64:72])[0]
fee_data = base64.b64decode(fixture[1]['data'][0])
assert struct.unpack_from('<I', fee_data, 65)[0] == 1
(_, protocol_bps, creator_bps) = struct.unpack_from('<QQQ', fee_data, 85)
net = 9900000 * 10000 // (10000 + protocol_bps + creator_bps)
net -= max(0, net + (net * protocol_bps + 9999) // 10000 + (net * creator_bps + 9999) // 10000 - 9900000)
expected = (net - 1) * 1073000000000000 // (30000000000 + net - 1)
if exact_out:
    expected = 100000000000
assert tokens == expected, (tokens, expected)
assert vm.get_account(recipient).lamports - initial_fee == 100000
spent = initial_sol - vm.get_account(u).lamports - 5000
assert 100000 <= spent <= 10000000, spent
print('ROUTER V3 BUY PASS', {'tokens': tokens, 'spent': spent, 'fee': 100000})
vm.set_account(ata(recipient, m), token_account(recipient, m, 0))
amount = tokens // 2
router_fee = amount // 100
initial_sol = vm.get_account(u).lamports
ix = route(bytes([28, 146, 222, 119, 38, 196, 105, 213]) + struct.pack('<QQ', amount - router_fee, 1), amount, 1, 1, u, ata(u, m), ata(recipient, m), T, S)
r = vm.send_transaction(Transaction.new_signed_with_payer([ix], u, [user], vm.latest_blockhash()))
if isinstance(r, FailedTransactionMetadata):
    print('SELL', r.err(), r.meta().logs())
    sys.exit(1)
assert struct.unpack('<Q', vm.get_account(ata(u, m)).data[64:72])[0] == tokens - amount
assert struct.unpack('<Q', vm.get_account(ata(recipient, m)).data[64:72])[0] == router_fee
received = vm.get_account(u).lamports - initial_sol + 5000
assert received > 0
print('ROUTER V3 SELL PASS', {'received_lamports': received, 'base_spent': amount, 'base_fee': router_fee})
