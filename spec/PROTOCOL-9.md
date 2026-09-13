# Paritr Protocol 9 — normative consensus specification

Status: release candidate 1. The words **MUST**, **MUST NOT**, **SHOULD** and **MAY** are normative. If this document and executable `src/consensus/` disagree before final release, that discrepancy is a release blocker; neither may be silently preferred.

## 1. Identity and primitives

| Name | Value |
|---|---|
| Chain ID | ASCII `paritr-mainnet` |
| Protocol | unsigned 16-bit `9` |
| Header version | unsigned 16-bit `3` |
| Atomic units per PAR | `100000000` |
| Maximum monetary field | `2^62 - 1` atomic units |
| PoW | RandomX v1 line, release 1.2.3 |

`u8`, `u16`, `u32` and `u64` are unsigned and encoded little-endian. A byte string is `u32(length) || bytes`. A vector starts with `u32(element_count)`; elements that can vary in length are themselves byte strings. Decoders MUST reject trailing bytes, invalid tags, non-minimal representations and lengths above the stated limits before allocating.

`SHA256d(x) = SHA256(SHA256(x))`. The domain-separated hash is:

```text
DH(domain, data) = SHA256(SHA256(
    LE32(len(domain)) || domain || LE64(len(data)) || data
))
```

Hash strings are lowercase hex in byte order. RandomX output is interpreted as an unsigned 256-bit **little-endian** integer for target comparison.

### Addresses and signatures

An address is exactly 25 bytes:

```text
55 || RIPEMD160(SHA256(uncompressed_SEC1_public_key)) ||
first4(SHA256d(previous_21_bytes))
```

Its text form is canonical Base58Check of those 25 bytes. This deliberately preserves P8 private keys and `P...` addresses. Transactions may carry a 33-byte compressed or 65-byte uncompressed secp256k1 public key, but address derivation always serializes it uncompressed. Signatures are 64-byte compact ECDSA `r || s`, MUST verify over the stated digest and MUST use low S.

## 2. Consensus objects

### Transaction v1

```text
u16 version (=1)
25B sender
25B recipient
u64 amount
u64 fee
u64 nonce
bytes public_key (33 or 65)
bytes signature (64)
```

Signing material is `bytes(chain_id) || u16(version) || sender || recipient || u64(amount) || u64(fee) || u64(nonce)`. The signed digest is `DH("PARITR-P9-TX-SIGN-v1", material)`. The transaction ID is `DH("PARITR-P9-TXID-v1", full_transaction_encoding)`. Maximum encoded size is 1,024 bytes.

A consensus-valid transaction MUST have a positive amount no greater than `MAX_MONEY`, fee no greater than `MAX_MONEY`, distinct sender/recipient, the account's exact current nonce, a funded `amount + fee`, the matching public key and valid low-S signature. On application, debit sender, credit amount to recipient, increment sender nonce by exactly one and assign the fee to the block finder. Consensus permits zero fee and amounts below local relay dust policy.

### RewardClaim v1

`u16 version || u64 height || 25B finder || u64 amount || u64 extranonce`. Its ID is `DH("PARITR-P9-REWARD-CLAIM-v1", encoding)`.

### Block header

The header is exactly 160 bytes:

```text
u16 version | u16 protocol | u64 height | 32B previous_block
32B transactions_root | 32B state_root | 32B workshare_root
u64 timestamp | u32 bits | u64 nonce
```

The block ID is `SHA256d(header_encoding)`. The RandomX input is exactly the same 160 header bytes.

### BlockTemplate v1

`u16 version || u64 height || 32B parent || u32 tx_count || repeated bytes(transaction)`. Template ID is `DH("PARITR-P9-TEMPLATE-v1", encoding)`. Maximum encoding is 4 MiB and maximum transactions is 4,096.

### Workshare v1

```text
u16 version
32B previous_workshare
32B template_id
25B miner
u64 extranonce
160B candidate_header
```

ID is `DH("PARITR-P9-WORKSHARE-v1", encoding)`. A witness is `u16 version (=1) || u32 template_count || repeated bytes(template) || u32 share_count || repeated bytes(workshare)` and its commitment is `DH("PARITR-P9-WORKSHARE-WITNESS-v1", encoding)`.

A canonical witness contains at most 512 templates and 512 Workshares and encodes to at most 4 MiB. Templates MUST be strictly sorted by template ID and their ID set MUST equal the set referenced by Workshares. Workshares MUST form one linear chain: the first points to zero and every later share points to the immediately preceding share.

### Block

```text
160B header
u8 reward_claim_tag (0 or 1), followed by RewardClaim when 1
u32 transaction_count, repeated bytes(transaction)
bytes(workshare_witness)
```

Non-genesis blocks MUST contain one RewardClaim. Maximum block encoding is 8 MiB and maximum transaction count is 4,096.

## 3. Commitments and state

Merkle empty root is `DH("PARITR-P9-MERKLE-EMPTY-v1", empty)`. Nonempty leaves are already 32-byte IDs. At each level, duplicate the final hash when odd and compute `DH("PARITR-P9-MERKLE-NODE-v1", left || right)`. A block's transaction leaves are RewardClaim ID first, then transaction IDs in block order.

State is a 256-level binary Sparse Merkle Tree traversed most-significant bit first over a 32-byte state-key hash.

- account key: `DH("PARITR-P9-STATE-KEY-v1", 0x00 || address)`;
- pending-reward key: `DH("PARITR-P9-STATE-KEY-v1", 0x01 || u64(maturity_height) || address)`;
- account value: `u64(balance) || u64(nonce)`;
- pending value: `u64(maturity_height) || address || u64(amount)`;
- empty leaf: `DH("PARITR-P9-SMT-EMPTY-LEAF-v1", empty)`;
- populated leaf: `DH("PARITR-P9-SMT-LEAF-v1", key || value)`;
- inner node: `DH("PARITR-P9-SMT-NODE-v1", left || right)`.

Empty hashes are recursively calculated from level 256 to 0. Default accounts and zero pending values are absent. At height `h`, state transition order is: (1) mature all pending entries whose maturity is `h`; (2) apply transactions sequentially; (3) calculate the exact reward allocation; (4) schedule all nonzero rewards at `h + 100`. The resulting root MUST equal `header.state_root`.

## 4. Issuance and Workshare rewards

For height 0 subsidy is zero. For height `h >= 1`:

```text
halvings = floor((h - 1) / 1,600,000)
subsidy = max(10 PAR >> halvings, 0.5 PAR)
```

Five percent of subsidy plus all current-block transaction fees goes to the finder. Ninety-five percent is a Workshare pool. It is allocated over Workshares contained in the preceding 1,440 L1 blocks (current witness excluded). Each share weight is `work(share_target)` from its candidate header bits. Per-address weights are summed.

If no eligible weight exists, the entire pool goes to the current finder. Otherwise each address initially receives `floor(pool * address_weight / total_weight)`. Remaining atomic units go one at a time to the largest fractional remainders, with lower binary address as the deterministic tie-break; final entries are address-sorted. Finder and Workshare rewards mature after exactly 100 blocks.

Every Workshare candidate header MUST reconstruct a fully valid candidate for the current parent and its referenced template: exact height, parent, bits, time after parent and median-time-past but no later than the settling block, correct RewardClaim derived from share miner/extranonce, transaction root, post-state root, and a `workshare_root` equal to the canonical prefix before that share. Its RandomX value MUST be `> block_target` and `<= share_target`; a full block solution cannot also claim Workshare credit.

## 5. Targets, ASERT and chain selection

Initial target is `2^242 - 1`; PoW limit is `2^248 - 1`. Compact `bits` uses the canonical Bitcoin-style unsigned exponent/mantissa form. Sign bit, zero mantissa, exponent 0 or >33, non-round-tripping forms, and exponent 33 with mantissa >`0xffff` are invalid.

The required target for a child depends only on the parent `(height, timestamp)`, not on the child's timestamp:

```text
ideal   = parent_height * 64
actual  = parent_timestamp - genesis_timestamp
drift   = actual - ideal
e       = floor_euclid(drift * 65536 / 7200)
shifts  = floor_euclid(e / 65536)
frac    = e mod_euclid 65536
factor  = 65536 + ((195766423245049*frac
                   + 971821376*frac^2
                   + 5127*frac^3
                   + 2^47) >> 48)
target  = initial_target * factor / 65536 * 2^shifts
target  = clamp(target, 1, pow_limit)
bits    = canonical_compact(target)
```

Target block interval is 64 seconds; ASERT half-life is 7,200 seconds. Share target is the canonical compact target obtained from `block_target * 32`, clamped to `2^256-1`, giving about a two-second expected interval.

`work(target) = floor((2^256 - 1 - target)/(target + 1)) + 1`. The valid chain with greatest sum of L1 block work is preferred. Workshares add no fork-choice work. Equal cumulative work chooses the lexicographically lower 32-byte tip ID. There is no consensus reorg-depth cap or emergency timestamp difficulty rule.

For every non-genesis header: height is parent+1, previous ID matches, bits equal the required bits and target does not exceed the PoW limit. Timestamp MUST be strictly greater than parent timestamp and median of the last up-to-11 block timestamps, and no more than 300 seconds ahead of validator time.

## 6. RandomX seed schedule

For candidate heights below 2,048 the seed bytes are ASCII `Paritr-Protocol-9-RandomX-Bootstrap-v1`. Thereafter:

```text
epoch = floor(height / 2048)
reference_height = epoch*2048 - 64
material = bytes(chain_id) || u64(epoch) || reference_block_id
seed = DH("PARITR-P9-RANDOMX-EPOCH-v1", material)
```

Nodes MUST use RandomX v1-compatible output. The mandatory loader self-test is key `test key 000`, input `This is a test`, expected output `639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f`.

## 7. Fixed genesis

The genesis is a checkpoint and is accepted only by byte-for-byte equality, not mined at startup.

| Field | Value |
|---|---|
| timestamp | `1788984000` |
| message committed by transactions_root | `Paritr Mainnet Protocol 9 - sustainable workshare PoW - 2026-09-09` |
| bits | `0x1f03ffff` (`520355839`) |
| nonce | `0` |
| transactions root | `ece9cf93862588e0de73602cd3361fa5c8efc031d5a9fafb65c3877b14e1d350` |
| state root | `e55ef31e65f0e301c8346f844a1bd748c395e23147881cec7a87173e147e10e9` |
| Workshare root | `e0622ca26dc9ce82101cd2dd43c17b21adea8108a92e27b0ccc78bf87b3148c0` |
| block ID | `44f076c3b96c8e7cb49605d13d04177cadb1f2e44faf9f5f2c249b5da32b320f` |

Exact bytes are in `test-vectors.json` and emitted by `paritr-node print-genesis`.

## 8. P2P and non-consensus policy

P2P frames begin ASCII magic `PRT9`, followed by a one-byte message tag and its deterministic payload. Maximum frame is `8 MiB + 64 KiB`. Peer Hellos contain protocol, chain ID, genesis, compressed node public key, timestamp, random challenge, `response_to`, height, tip, cumulative work, advertised URL and compact signature over `DH("PARITR-P9-HELLO-v1", hello_without_signature)`. Timestamps are within 300 seconds. The server sends an initial Hello with zero `response_to`; client signs a Hello responding to its challenge; server signs an acknowledgement responding to the client's challenge. No peer is admitted before this exchange.

Header synchronization uses exponential block locators, batches of at most 2,000 headers and sequential full-block retrieval/validation. Peers MUST NOT treat unvalidated header work as chain work. Missing Workshare templates and predecessors are requested; orphan memory is bounded.

Relay policy is explicitly non-consensus: default minimum relay fee 100,000 atomic units, dust 1,000, mempool 64 MiB, nonce gap 64, TTL 24 hours, and same-nonce replacement requiring the greater of 100,000 or a 10% fee increment. The reference node additionally caps the live template cache at 512 objects/64 MiB, live Workshares at 8,192, dependency orphans at 2,048, and each peer at 1,000 messages including 200 object-validation messages per fixed ten-second window. A miner MAY use different policy but MUST accept any block satisfying consensus.
