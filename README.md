# Mining Tycoon - Solana Mining Game

A sustainable yield farming protocol on Solana where users buy MH/s (mining power) that generates hash power, compound for exponential growth, or sell for SOL.

## Overview

**Mining Tycoon** is a fork of the original BakedBeans BSC miner, adapted for Solana using the Anchor framework with improved terminology and sustainable economics.

### Key Features

- **Buy MH/s**: Purchase mining power with SOL
- **Compound**: Convert accumulated hash power into more MH/s
- **Sell**: Exchange hash power for SOL anytime
- **Referral System**: 5% bonus for referrers
- **Sustainable Growth**: Battle-tested parameters from original BNB model

## Economics

### Parameters (Original BNB Model)

- **Hash to MH/s**: 1,080,000 hash = 1 new MH/s (~12.5 days)
- **Market GPUs**: 108 billion (sustainable pricing)
- **Protocol Fee**: 10% (used for $GPU buyback & burn - see below)
- **Referral Bonus**: 5%
- **Virtual TVL Offset**: 100 SOL (prevents early advantage)
- **Bonding Curve**: PSN=5,000, PSNH=10,000 (balanced)

### Protocol Fee & $GPU Token Integration

The 10% protocol fee serves a dual purpose in the Mining Tycoon ecosystem:

1. **Revenue Generation**: Collected on all buy and sell transactions
2. **$GPU Token Utility**: 100% of protocol fees are used to buy back and burn $GPU tokens

**How It Works**:
- When users buy MH/s or sell hash power, 10% fee goes to protocol wallet
- Protocol wallet periodically uses accumulated SOL to buy $GPU from the market
- Purchased $GPU tokens are permanently burned (sent to dead address)
- This creates continuous buy pressure and reduces $GPU supply
- **Result**: Deflationary mechanism that increases $GPU scarcity over time

**Benefits**:
- ✅ Aligns game growth with $GPU token value
- ✅ More activity in Mining Tycoon = More $GPU burns
- ✅ Sustainable tokenomics (not inflationary)
- ✅ Incentivizes long-term holding of $GPU

**Example**:
```
User buys 1 SOL of MH/s → 0.1 SOL protocol fee collected
Protocol wallet accumulates fees → Buys $GPU from market
Purchased $GPU → Burned permanently
Supply decreases → Scarcity increases → Value potential rises
```

This mechanism ensures that as Mining Tycoon grows in popularity, $GPU token becomes increasingly scarce, creating a positive feedback loop between the game and the token economy.

### How It Works

1. **Each MH/s generates 1 hash per second**
2. **Accumulate 1,080,000 hash → compound to get 1 new MH/s**
3. **Max accumulation**: 12.5 days worth of hash
4. **Compound regularly** for exponential growth
5. **Sell anytime** for SOL (10% fee)

## Smart Contract Structure

### State Accounts

**GlobalState**:
- `market_gpus`: u64 - Total GPU market supply
- `hashpower_to_hire_1miner`: u64 - Hash needed for 1 MH/s
- `protocol_fee_val`: u8 - Protocol fee percentage (used for $GPU buyback/burn)
- `dev_wallet`: Pubkey - Protocol wallet that receives fees for $GPU burns
- `psn/psnh`: u64 - Bonding curve parameters

**UserState**:
- `mining_power`: u64 - User's MH/s
- `accumulated_hashpower`: u64 - Accumulated hash
- `last_compound`: i64 - Last compound timestamp
- `referrer`: Option<Pubkey> - Referrer address

### Main Instructions

1. `initialize(seed_amount, dev_wallet)` - Initialize program (admin only)
2. `buy_mining_power(amount, referrer)` - Buy MH/s with SOL
3. `compound_hashpower(referrer)` - Compound hash to MH/s
4. `sell_hashpower()` - Sell hash for SOL
5. `init_user()` - Initialize user account

### Admin Functions

- `update_hashpower_requirement(new_value)` - Adjust hash per MH/s
- `update_market_gpus(new_value)` - Adjust market supply
- `multiply_user_mining_power()` - 10x boost for specific user

## Building

```bash
anchor build
```

## Testing

```bash
anchor test
```

## Deployment

1. Update `Anchor.toml` with your program ID
2. Build: `anchor build`
3. Deploy: `solana program deploy`
4. Initialize with dev wallet address

## Security Considerations

- All admin functions require authority signature
- User state is PDA-derived (cannot be spoofed)
- Vault uses PDA signer for secure transfers
- Overflow/underflow protection on all math
- Virtual TVL offset prevents early whale advantage

## License

MIT

## Credits

Based on the original BakedBeans BSC miner concept, adapted for Solana with sustainable parameters and improved terminology.
