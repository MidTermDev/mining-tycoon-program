# Mining Tycoon V2 - Dual Currency Solana Mining Game

A dual currency yield farming protocol on Solana where users buy MH/s (mining power) with SOL or $GPU tokens, generate hash continuously, and earn from BOTH SOL and GPU pools simultaneously.

## Overview

**Mining Tycoon V2** uses a revolutionary dual currency hybrid model:
- **Dual payment options**: Buy with SOL or $GPU tokens (15% GPU penalty)
- **Dual vault system**: Separate SOL and GPU pools
- **Dual rewards**: Earn from BOTH pools based on your MH/s share
- **TVL-scaled buying** (prevents whale advantage)
- **Hash generation** (MH/s produces hash continuously)
- **Compound or Claim**: Compound (no fee) or Claim both currencies (10% fee)
- **Price oracle**: Live SOL/GPU price updates every 20 seconds

### Key Innovation

The hybrid model solves the broken economics of bonding curves by separating buy and sell mechanics:
- **Buy MH/s**: Rate scales with TVL (formula independent)
- **Earn from pool**: Your share of daily 10% TVL
- **Compound hash**: Direct conversion to MH/s (no fee!)
- **Claim SOL**: Get your pool share (10% protocol fee)

## Economics

### Hybrid Model Parameters

**Buy MH/s** (TVL-Scaled):
- Base rate: 1000 MH/s per SOL at TVL=1
- Formula: `(lamports × 1000 × 100) / (100e9 + vault_lamports)`
- As TVL grows, buy rate decreases (but not extremely)
- Example: 0.01 SOL → ~10 MH/s at TVL=1

**Generate Hash**:
- 1 MH/s = 1 hash per second
- Accumulates continuously
- No maximum cap

**Compound Hash** (No Fee!):
- 86,400 hash = 1 new MH/s (1 day)
- Direct conversion, no protocol fee
- Best for exponential growth
- Incentivized strategy

**Claim SOL** (10% Fee):
- Mining pool share: `(Your MH/s / Total MH/s) × (10% of TVL per day)`
- Time-based accumulation
- 10% protocol fee → $GPU buyback & burn
- Get SOL immediately

### Protocol Fee & $GPU Token

The 10% protocol fee on claimed SOL serves dual purpose:

1. **Revenue Generation**: Collected when users claim SOL
2. **$GPU Utility**: 100% used to buy back and burn $GPU tokens

**Deflationary Mechanism**:
```
User claims SOL → 10% protocol fee collected
Protocol accumulates fees → Buys $GPU from market  
Purchased $GPU → Burned permanently
Supply decreases → Scarcity increases
```

**Benefits**:
- ✅ Game growth = $GPU scarcity
- ✅ Sustainable tokenomics
- ✅ Incentivizes compounding (fee-free)
- ✅ Creates positive feedback loop

### Why Compounding is Better

**Compound**: 86,400 hash → 1 MH/s (NO FEE)
**Claim**: 86,400 hash → 0.X SOL → Pay 10% fee → Buy back MH/s = LESS MH/s

Compounding gives you MORE mining power for the same hash!

## Smart Contract Structure

### State Accounts

**GlobalState**:
- `total_mining_power`: u64 - Total MH/s in ecosystem
- `total_unclaimed_sol`: u64 - Unclaimed SOL (excluded from mineable TVL)
- `total_unclaimed_gpu`: u64 - Unclaimed GPU (excluded from mineable TVL)
- `daily_pool_percentage`: u8 - % of TVL mineable per day (10%)
- `base_buy_rate`: u64 - MH/s per SOL at TVL=1 (1000)
- `protocol_fee_val`: u8 - Protocol fee (10%)
- `gpu_penalty_bps`: u16 - GPU buy penalty (1500 = 15%)
- `sol_usd_price`: u64 - SOL price in USD (8 decimals)
- `gpu_usd_price`: u64 - GPU price in USD (8 decimals)
- `gpu_token_mint`: Pubkey - GPU token address (configurable)
- `dev_wallet`: Pubkey - Receives protocol fees

**UserState**:
- `mining_power`: u64 - User's MH/s
- `unclaimed_earnings`: u64 - Accumulated hash/SOL
- `unclaimed_gpu_earnings`: u64 - Accumulated GPU
- `last_claim`: i64 - Last claim/compound timestamp
- `referrer`: Option<Pubkey> - Referrer address
- `total_sol_claimed`: u64 - Lifetime SOL claimed
- `total_gpu_claimed`: u64 - Lifetime GPU claimed

### Main Instructions

**Core Functions**:
1. `initialize(seed_amount, dev_wallet)` - Initialize program
2. `buy_mining_power(amount, referrer)` - Buy MH/s with SOL
3. `buy_with_gpu(amount, referrer)` - Buy MH/s with $GPU tokens (15% penalty)
4. `compound_hash()` - Convert hash → MH/s (no fee!)
5. `claim_earnings()` - Claim both SOL and GPU from pools (10% fee each)
6. `init_user()` - Initialize user account

**Admin Functions**:
1. `update_prices(sol_usd, gpu_usd)` - Update price oracle
2. `set_gpu_token(token_mint)` - Change GPU token address

**View Functions**:
1. `get_mhs_quote(sol_amount)` - Get exact MH/s quote for SOL amount

## How It Works

### 1. Buy MH/s

User deposits SOL → Gets MH/s based on TVL-scaled rate

```rust
MH/s = (lamports × 1000 × 100) / (100e9 + vault_lamports)
```

### 2. Generate Hash

Each MH/s generates 1 hash per second automatically.

### 3. Use Hash

**Option A: Compound** (Recommended!)
- 86,400 hash → 1 MH/s
- No protocol fee
- Exponential growth
- Better long-term ROI

**Option B: Claim SOL**
- Get share of daily mining pool
- Formula: `(Your MH/s / Total MH/s) × (10% TVL per day) × time`
- 10% protocol fee
- Immediate SOL

### 4. Unclaimed Tracking

Unclaimed SOL is excluded from mineable TVL to prevent:
- Bank runs
- Negative balance scenarios
- Unsustainable payouts

## Building

```bash
anchor build
```

## Deployment

```bash
anchor deploy
```

Then initialize:
```bash
anchor run initialize --provider.cluster mainnet
```

## Security

- User state is PDA-derived (cannot be spoofed)
- Vault uses PDA signer for secure transfers
- Overflow/underflow protection on all math
- Unclaimed SOL tracking prevents bank runs
- No admin functions in public version

## Dual Currency Features

### Buy with SOL or $GPU
- **SOL**: Standard bonding curve rate
- **$GPU**: 15% penalty applied (costs more per MH/s in USD terms)
- Users can choose their payment method

### Dual Pool Earnings
- **SOL Pool**: 10% of SOL vault daily
- **GPU Pool**: 10% of GPU vault daily
- Users earn from BOTH simultaneously based on MH/s share
- Claim distributes both currencies at once

### Price Oracle
- Updates SOL and GPU USD prices every 20 seconds
- Used for accurate MH/s calculations when buying with GPU
- Ensures fair conversion rates

### Configurable GPU Token
- Admin can change GPU token address anytime
- Allows switching to different tokens
- Separate vaults for each token

## Live Deployment

**Program ID**: `t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU`
**Website**: MiningTycoon.fun

## License

MIT

## Credits

Innovative hybrid model combining best of bonding curves and mining pools, adapted for Solana.
