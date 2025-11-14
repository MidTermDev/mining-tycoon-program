use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, Transfer as SplTransfer};
use anchor_spl::token_interface::TokenAccount;

declare_id!("t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU");

// GPU Token Decimals (constant since most tokens use 6 or 9)
pub const GPU_TOKEN_DECIMALS: u8 = 6;

#[program]
pub mod bakedbeans_solana {
    use super::*;

    /// Initialize with new mining pool model - now with dual currency support
    pub fn initialize(ctx: Context<Initialize>, seed_amount: u64, dev_wallet: Pubkey) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        
        require!(seed_amount > 0, ErrorCode::InvalidSeedAmount);
        
        global_state.authority = ctx.accounts.authority.key();
        global_state.dev_wallet = dev_wallet;
        global_state.total_mining_power = 0;
        global_state.total_unclaimed_sol = 0;
        global_state.total_unclaimed_gpu = 0;
        global_state.initialized = true;
        global_state.daily_pool_percentage = 10; // 10% of TVL per day
        global_state.base_buy_rate = 1000; // 1000 MH/s per SOL at TVL=1
        global_state.protocol_fee_val = 10;
        global_state.gpu_penalty_bps = 1500; // 15% penalty for GPU buys
        global_state.sol_usd_price = 0; // Will be set by admin
        global_state.gpu_usd_price = 0; // Will be set by admin
        global_state.gpu_token_mint = Pubkey::default(); // Will be set by admin
        
        msg!("Mining Tycoon v2 initialized - Dual Currency Mining Pool Model");
        
        Ok(())
    }

    /// Buy MH/s with SOL - NEW: 1% of hashrate = 2% of TVL
    /// SECURITY FIX: SOL transfer happens via CPI to prevent exploit
    pub fn buy_mining_power(ctx: Context<BuyMiningPower>, amount: u64, referrer: Option<Pubkey>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(global_state.sol_usd_price > 0, ErrorCode::PriceNotSet);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        if user_state.referrer.is_none() && referrer.is_some() {
            let ref_key = referrer.unwrap();
            require!(ref_key != ctx.accounts.buyer.key(), ErrorCode::SelfReferral);
            user_state.referrer = Some(ref_key);
        }
        
        // SECURITY FIX: Transfer SOL via CPI to ensure payment actually happens
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.buyer.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;
        
        // NEW PRICING MODEL: Calculate based on TVL percentage
        // Convert SOL to USD
        let sol_usd_value = (amount as u128)
            .checked_mul(global_state.sol_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(1_000_000_000) // SOL has 9 decimals, price has 8
            .ok_or(ErrorCode::DivisionByZero)?;
        
        // Calculate total TVL in USD (SOL + GPU)
        let vault_balance = ctx.accounts.vault.to_account_info().lamports();
        let sol_tvl_usd = (vault_balance as u128)
            .checked_mul(global_state.sol_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(1_000_000_000)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        // Read GPU vault balance from token account data
        let gpu_vault_balance = if ctx.accounts.gpu_vault.data_len() >= 72 {
            let data = ctx.accounts.gpu_vault.try_borrow_data()?;
            u64::from_le_bytes(data[64..72].try_into().unwrap_or([0; 8]))
        } else {
            0
        };
        
        let gpu_tvl_usd = (gpu_vault_balance as u128)
            .checked_mul(global_state.gpu_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(1_000_000) // GPU has 6 decimals, price has 8
            .ok_or(ErrorCode::DivisionByZero)?;
        
        let total_tvl_usd = sol_tvl_usd.checked_add(gpu_tvl_usd).ok_or(ErrorCode::Overflow)?;
        
        // Calculate MH/s using new model
        let mhs_bought = calculate_mhs_for_usd(
            sol_usd_value,
            global_state.total_mining_power,
            total_tvl_usd
        )?;
        
        // Apply protocol fee
        let fee_mhs = mhs_bought.checked_mul(global_state.protocol_fee_val as u64)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100)
            .ok_or(ErrorCode::DivisionByZero)?;
        let mhs_after_fee = mhs_bought.checked_sub(fee_mhs).ok_or(ErrorCode::Overflow)?;
        
        // Update user and global state
        user_state.mining_power = user_state.mining_power
            .checked_add(mhs_after_fee)
            .ok_or(ErrorCode::Overflow)?;
        user_state.last_claim = clock.unix_timestamp;
        
        global_state.total_mining_power = global_state.total_mining_power
            .checked_add(mhs_after_fee)
            .ok_or(ErrorCode::Overflow)?;
        
        // Referral bonus (5%) - skip if no referrer or referrer PDA doesn't exist
        if let Some(ref_account_info) = &ctx.accounts.referrer_state {
            // Only process if account is initialized (owned by our program)
            if *ref_account_info.owner == *ctx.program_id && ref_account_info.data_len() >= 8 {
                // Manually deserialize UserState
                let data = ref_account_info.try_borrow_data()?;
                if data.len() >= 40 { // At least has mining_power field
                    let current_power_bytes: [u8; 8] = data[40..48].try_into().map_err(|_| ErrorCode::Overflow)?;
                    let current_power = u64::from_le_bytes(current_power_bytes);
                    
                    let referral_bonus = mhs_after_fee.checked_div(20).ok_or(ErrorCode::DivisionByZero)?;
                    let new_power = current_power.checked_add(referral_bonus).ok_or(ErrorCode::Overflow)?;
                    
                    // Write back (very unsafe, but needed for optional account)
                    drop(data);
                    let mut data = ref_account_info.try_borrow_mut_data()?;
                    data[40..48].copy_from_slice(&new_power.to_le_bytes());
                    
                    global_state.total_mining_power = global_state.total_mining_power
                        .checked_add(referral_bonus)
                        .ok_or(ErrorCode::Overflow)?;
                    msg!("Sent {} MH/s to referrer", referral_bonus);
                }
            }
        }
        
        msg!("Bought {} MH/s for {} lamports", mhs_after_fee, amount);
        
        Ok(())
    }

    /// Buy MH/s with GPU token (15% penalty)
    pub fn buy_with_gpu(ctx: Context<BuyWithGpu>, amount: u64, referrer: Option<Pubkey>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(global_state.gpu_usd_price > 0, ErrorCode::PriceNotSet);
        require!(global_state.sol_usd_price > 0, ErrorCode::PriceNotSet);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        if user_state.referrer.is_none() && referrer.is_some() {
            let ref_key = referrer.unwrap();
            require!(ref_key != ctx.accounts.buyer.key(), ErrorCode::SelfReferral);
            user_state.referrer = Some(ref_key);
        }
        
        // Convert GPU amount to USD equivalent
        let gpu_usd_value = (amount as u128)
            .checked_mul(global_state.gpu_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(10u128.pow(GPU_TOKEN_DECIMALS as u32))
            .ok_or(ErrorCode::DivisionByZero)?;
        
        // Convert to SOL equivalent based on USD value
        let sol_equivalent = gpu_usd_value
            .checked_mul(10u128.pow(9)) // SOL has 9 decimals
            .ok_or(ErrorCode::Overflow)?
            .checked_div(global_state.sol_usd_price as u128)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        let sol_amount = u64::try_from(sol_equivalent).map_err(|_| ErrorCode::Overflow)?;
        
        // Apply 15% penalty (GPU buyers pay more)
        let gpu_usd_with_penalty = gpu_usd_value
            .checked_mul(10000 + global_state.gpu_penalty_bps as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(10000)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        // Calculate total TVL in USD (SOL + GPU)
        let sol_vault_balance = ctx.accounts.sol_vault.to_account_info().lamports();
        let sol_tvl_usd = (sol_vault_balance as u128)
            .checked_mul(global_state.sol_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(1_000_000_000)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        let gpu_vault_balance = ctx.accounts.gpu_vault.amount;
        let gpu_tvl_usd = (gpu_vault_balance as u128)
            .checked_mul(global_state.gpu_usd_price as u128)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(1_000_000)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        let total_tvl_usd = sol_tvl_usd.checked_add(gpu_tvl_usd).ok_or(ErrorCode::Overflow)?;
        
        // Calculate MH/s using new pricing model (after penalty)
        let mhs_bought = calculate_mhs_for_usd(
            gpu_usd_with_penalty,
            global_state.total_mining_power,
            total_tvl_usd
        )?;
        
        // Apply protocol fee
        let fee_mhs = mhs_bought.checked_mul(global_state.protocol_fee_val as u64)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100)
            .ok_or(ErrorCode::DivisionByZero)?;
        let mhs_after_fee = mhs_bought.checked_sub(fee_mhs).ok_or(ErrorCode::Overflow)?;
        
        // Update user and global state
        user_state.mining_power = user_state.mining_power
            .checked_add(mhs_after_fee)
            .ok_or(ErrorCode::Overflow)?;
        user_state.last_claim = clock.unix_timestamp;
        
        global_state.total_mining_power = global_state.total_mining_power
            .checked_add(mhs_after_fee)
            .ok_or(ErrorCode::Overflow)?;
        
        // Referral bonus (5%)
        if let Some(referrer_state) = ctx.accounts.referrer_state.as_mut() {
            let referral_bonus = mhs_after_fee.checked_div(20).ok_or(ErrorCode::DivisionByZero)?;
            referrer_state.mining_power = referrer_state.mining_power
                .checked_add(referral_bonus)
                .ok_or(ErrorCode::Overflow)?;
            global_state.total_mining_power = global_state.total_mining_power
                .checked_add(referral_bonus)
                .ok_or(ErrorCode::Overflow)?;
            msg!("Sent {} MH/s to referrer", referral_bonus);
        }
        
        // Transfer GPU tokens to vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                SplTransfer {
                    from: ctx.accounts.buyer_gpu_account.to_account_info(),
                    to: ctx.accounts.gpu_vault.to_account_info(),
                    authority: ctx.accounts.buyer.to_account_info(),
                },
            ),
            amount,
        )?;
        
        msg!("Bought {} MH/s with {} GPU tokens (penalty applied)", mhs_after_fee, amount);
        
        Ok(())
    }

    /// Compound hash into more MH/s (no fee - better than claiming!)
    pub fn compound_hash(ctx: Context<CompoundHash>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        require!(user_state.mining_power > 0, ErrorCode::InvalidAmount);
        
        // Calculate hash generated since last claim
        let time_passed = (clock.unix_timestamp - user_state.last_claim) as u64;
        let hash_generated = time_passed.checked_mul(user_state.mining_power).ok_or(ErrorCode::Overflow)?;
        
        // Add stored unclaimed hash
        let total_hash = user_state.unclaimed_earnings.checked_add(hash_generated).ok_or(ErrorCode::Overflow)?;
        require!(total_hash > 0, ErrorCode::InvalidAmount);
        
        // Convert hash to MH/s (no fee!)
        // Use simple rate: 86,400 hash = 1 MH/s (1 day)
        let new_mhs = total_hash / 86_400;
        require!(new_mhs > 0, ErrorCode::InvalidAmount);
        
        // Update state
        user_state.mining_power = user_state.mining_power.checked_add(new_mhs).ok_or(ErrorCode::Overflow)?;
        user_state.unclaimed_earnings = 0;
        user_state.last_claim = clock.unix_timestamp;
        
        global_state.total_mining_power = global_state.total_mining_power.checked_add(new_mhs).ok_or(ErrorCode::Overflow)?;
        
        msg!("Compounded {} hash into {} MH/s (no fee!)", total_hash, new_mhs);
        
        Ok(())
    }

    /// Claim accumulated SOL and GPU earnings (dual currency)
    pub fn claim_earnings(ctx: Context<ClaimEarnings>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        require!(user_state.mining_power > 0, ErrorCode::InvalidAmount);
        
        // Calculate new SOL earnings
        let sol_vault_balance = ctx.accounts.sol_vault.to_account_info().lamports();
        let mineable_sol_tvl = sol_vault_balance.checked_sub(global_state.total_unclaimed_sol).ok_or(ErrorCode::Overflow)?;
        
        let new_sol_earnings = calculate_earnings(
            user_state.mining_power,
            global_state.total_mining_power,
            user_state.last_claim,
            clock.unix_timestamp,
            mineable_sol_tvl,
            global_state.daily_pool_percentage
        )?;
        
        // Calculate new GPU earnings
        let gpu_vault_balance = ctx.accounts.gpu_vault.amount;
        let mineable_gpu_tvl = gpu_vault_balance.checked_sub(global_state.total_unclaimed_gpu).ok_or(ErrorCode::Overflow)?;
        
        let new_gpu_earnings = calculate_earnings(
            user_state.mining_power,
            global_state.total_mining_power,
            user_state.last_claim,
            clock.unix_timestamp,
            mineable_gpu_tvl,
            global_state.daily_pool_percentage
        )?;
        
        // Add to unclaimed
        user_state.unclaimed_earnings = user_state.unclaimed_earnings
            .checked_add(new_sol_earnings)
            .ok_or(ErrorCode::Overflow)?;
        user_state.unclaimed_gpu_earnings = user_state.unclaimed_gpu_earnings
            .checked_add(new_gpu_earnings)
            .ok_or(ErrorCode::Overflow)?;
        
        global_state.total_unclaimed_sol = global_state.total_unclaimed_sol
            .checked_add(new_sol_earnings)
            .ok_or(ErrorCode::Overflow)?;
        global_state.total_unclaimed_gpu = global_state.total_unclaimed_gpu
            .checked_add(new_gpu_earnings)
            .ok_or(ErrorCode::Overflow)?;
        
        user_state.last_claim = clock.unix_timestamp;
        
        // Claim all unclaimed SOL
        let total_sol_to_claim = user_state.unclaimed_earnings;
        let total_gpu_to_claim = user_state.unclaimed_gpu_earnings;
        
        require!(total_sol_to_claim > 0 || total_gpu_to_claim > 0, ErrorCode::InvalidAmount);
        
        // Process SOL claim
        if total_sol_to_claim > 0 {
            require!(sol_vault_balance >= total_sol_to_claim, ErrorCode::InsufficientFunds);
            
            let sol_fee = total_sol_to_claim.checked_mul(global_state.protocol_fee_val as u64)
                .ok_or(ErrorCode::Overflow)?
                .checked_div(100)
                .ok_or(ErrorCode::DivisionByZero)?;
            let sol_payout = total_sol_to_claim.checked_sub(sol_fee).ok_or(ErrorCode::Overflow)?;
            
            user_state.unclaimed_earnings = 0;
            user_state.total_sol_claimed = user_state.total_sol_claimed
                .checked_add(sol_payout)
                .ok_or(ErrorCode::Overflow)?;
            
            global_state.total_unclaimed_sol = global_state.total_unclaimed_sol
                .checked_sub(total_sol_to_claim)
                .ok_or(ErrorCode::Overflow)?;
            
            let vault_bump = ctx.bumps.sol_vault;
            let signer_seeds: &[&[&[u8]]] = &[&[b"vault", &[vault_bump]]];
            
            anchor_lang::system_program::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_lang::system_program::Transfer {
                        from: ctx.accounts.sol_vault.to_account_info(),
                        to: ctx.accounts.user.to_account_info(),
                    },
                    signer_seeds,
                ),
                sol_payout,
            )?;
            
            anchor_lang::system_program::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    anchor_lang::system_program::Transfer {
                        from: ctx.accounts.sol_vault.to_account_info(),
                        to: ctx.accounts.dev_wallet.to_account_info(),
                    },
                    signer_seeds,
                ),
                sol_fee,
            )?;
            
            msg!("Claimed {} SOL (fee: {})", sol_payout, sol_fee);
        }
        
        // Process GPU claim
        if total_gpu_to_claim > 0 {
            require!(gpu_vault_balance >= total_gpu_to_claim, ErrorCode::InsufficientFunds);
            
            let gpu_fee = total_gpu_to_claim.checked_mul(global_state.protocol_fee_val as u64)
                .ok_or(ErrorCode::Overflow)?
                .checked_div(100)
                .ok_or(ErrorCode::DivisionByZero)?;
            let gpu_payout = total_gpu_to_claim.checked_sub(gpu_fee).ok_or(ErrorCode::Overflow)?;
            
            user_state.unclaimed_gpu_earnings = 0;
            user_state.total_gpu_claimed = user_state.total_gpu_claimed
                .checked_add(gpu_payout)
                .ok_or(ErrorCode::Overflow)?;
            
            global_state.total_unclaimed_gpu = global_state.total_unclaimed_gpu
                .checked_sub(total_gpu_to_claim)
                .ok_or(ErrorCode::Overflow)?;
            
            let gpu_vault_bump = ctx.bumps.gpu_vault_authority;
            let gpu_signer_seeds: &[&[&[u8]]] = &[&[b"gpu_vault", &[gpu_vault_bump]]];
            
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    SplTransfer {
                        from: ctx.accounts.gpu_vault.to_account_info(),
                        to: ctx.accounts.user_gpu_account.to_account_info(),
                        authority: ctx.accounts.gpu_vault_authority.to_account_info(),
                    },
                    gpu_signer_seeds,
                ),
                gpu_payout,
            )?;
            
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    SplTransfer {
                        from: ctx.accounts.gpu_vault.to_account_info(),
                        to: ctx.accounts.dev_gpu_account.to_account_info(),
                        authority: ctx.accounts.gpu_vault_authority.to_account_info(),
                    },
                    gpu_signer_seeds,
                ),
                gpu_fee,
            )?;
            
            msg!("Claimed {} GPU tokens (fee: {})", gpu_payout, gpu_fee);
        }
        
        Ok(())
    }

    /// Initialize user account
    pub fn init_user(ctx: Context<InitUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        user_state.owner = ctx.accounts.user.key();
        user_state.mining_power = 0;
        user_state.unclaimed_earnings = 0;
        user_state.unclaimed_gpu_earnings = 0;
        user_state.last_claim = clock.unix_timestamp;
        user_state.referrer = None;
        user_state.total_sol_claimed = 0;
        user_state.total_gpu_claimed = 0;
        
        msg!("User initialized");
        
        Ok(())
    }

    /// Admin: Drain vault
    pub fn drain_vault(ctx: Context<DrainVault>, amount: u64) -> Result<()> {
        let vault_bump = ctx.bumps.vault;
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", &[vault_bump]]];
        
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.authority.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;
        
        msg!("Drained {} lamports from vault", amount);
        
        Ok(())
    }

    /// Admin: Reset user mining power
    pub fn reset_user_power(ctx: Context<ResetUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        let old_power = user_state.mining_power;
        
        // Subtract from global total
        ctx.accounts.global_state.total_mining_power = ctx.accounts.global_state.total_mining_power
            .checked_sub(old_power)
            .ok_or(ErrorCode::Overflow)?;
        
        user_state.mining_power = 0;
        user_state.unclaimed_earnings = 0;
        user_state.unclaimed_gpu_earnings = 0;
        
        msg!("Reset user mining power from {} to 0", old_power);
        
        Ok(())
    }

    /// Admin: Update price oracle (SOL and GPU prices in USD with 8 decimals)
    pub fn update_prices(ctx: Context<UpdatePrices>, sol_usd_price: u64, gpu_usd_price: u64) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        
        require!(sol_usd_price > 0, ErrorCode::InvalidAmount);
        require!(gpu_usd_price > 0, ErrorCode::InvalidAmount);
        
        global_state.sol_usd_price = sol_usd_price;
        global_state.gpu_usd_price = gpu_usd_price;
        
        msg!("Updated prices - SOL: ${}, GPU: ${}", 
            sol_usd_price as f64 / 100_000_000.0,
            gpu_usd_price as f64 / 100_000_000.0
        );
        
        Ok(())
    }

    /// Admin: Set GPU token mint address (for testing different tokens)
    pub fn set_gpu_token(ctx: Context<SetGpuToken>, gpu_token_mint: Pubkey) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        
        require!(gpu_token_mint != Pubkey::default(), ErrorCode::InvalidAmount);
        
        global_state.gpu_token_mint = gpu_token_mint;
        
        msg!("GPU token mint updated to: {}", gpu_token_mint);
        
        Ok(())
    }
}

// NEW PRICING: 1% of network hashrate = 2% of total TVL (in USD)
// Calculate MH/s based on TVL percentage model
fn calculate_mhs_for_usd(usd_amount: u128, total_mhs: u64, tvl_usd: u128) -> Result<u64> {
    // 1% of hashrate costs 2% of TVL
    // MH/s = (USD / TVL) × total_MH/s × 0.5
    // Rearranged: MH/s = (USD × total_MH/s) / (TVL × 2)
    
    if tvl_usd == 0 || total_mhs == 0 {
        return Ok(0);
    }
    
    let numerator = usd_amount.checked_mul(total_mhs as u128).ok_or(ErrorCode::Overflow)?;
    let denominator = tvl_usd.checked_mul(2).ok_or(ErrorCode::Overflow)?;
    let mhs = numerator.checked_div(denominator).ok_or(ErrorCode::DivisionByZero)?;
    
    u64::try_from(mhs).map_err(|_| ErrorCode::Overflow.into())
}

fn calculate_earnings(
    user_mhs: u64,
    total_mhs: u64,
    last_claim: i64,
    current_time: i64,
    vault_balance: u64,
    daily_percentage: u8
) -> Result<u64> {
    if total_mhs == 0 {
        return Ok(0);
    }
    
    let time_passed = (current_time - last_claim) as u64;
    let seconds_in_day = 86_400u64;
    
    // User's share of total mining power
    let user_share_numerator = (user_mhs as u128).checked_mul(1_000_000).ok_or(ErrorCode::Overflow)?;
    let user_share = user_share_numerator.checked_div(total_mhs as u128).ok_or(ErrorCode::DivisionByZero)?;
    
    // Daily pool = daily_percentage% of vault
    let daily_pool = (vault_balance as u128)
        .checked_mul(daily_percentage as u128)
        .ok_or(ErrorCode::Overflow)?
        .checked_div(100)
        .ok_or(ErrorCode::DivisionByZero)?;
    
    // Earnings = user's share of pool, pro-rated by time
    let earnings = daily_pool
        .checked_mul(user_share)
        .ok_or(ErrorCode::Overflow)?
        .checked_div(1_000_000)
        .ok_or(ErrorCode::DivisionByZero)?
        .checked_mul(time_passed as u128)
        .ok_or(ErrorCode::Overflow)?
        .checked_div(seconds_in_day as u128)
        .ok_or(ErrorCode::DivisionByZero)?;
    
    u64::try_from(earnings).map_err(|_| ErrorCode::Overflow.into())
}

// Rest of structs...
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + GlobalState::INIT_SPACE,
        seeds = [b"global_state"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    /// CHECK: Vault
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitUser<'info> {
    #[account(
        init,
        payer = user,
        space = 8 + UserState::INIT_SPACE,
        seeds = [b"user_state", user.key().as_ref()],
        bump
    )]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyWithGpu<'info> {
    #[account(mut, seeds = [b"global_state"], bump)]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut, seeds = [b"user_state", buyer.key().as_ref()], bump)]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    /// CHECK: SOL Vault (for TVL calculation)
    #[account(mut, seeds = [b"vault"], bump)]
    pub sol_vault: AccountInfo<'info>,
    
    #[account(mut)]
    pub gpu_vault: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub buyer_gpu_account: InterfaceAccount<'info, TokenAccount>,
    
    /// CHECK: Optional referrer
    #[account(mut)]
    pub referrer_state: Option<Account<'info, UserState>>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyMiningPower<'info> {
    #[account(mut, seeds = [b"global_state"], bump)]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut, seeds = [b"user_state", buyer.key().as_ref()], bump)]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    /// CHECK: Vault
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,
    
    /// CHECK: GPU Vault ATA (for TVL calculation only)
    pub gpu_vault: AccountInfo<'info>,
    
    /// CHECK: Dev wallet
    #[account(mut, address = global_state.dev_wallet)]
    pub dev_wallet: AccountInfo<'info>,
    
    /// CHECK: Optional referrer (unchecked to allow null)
    pub referrer_state: Option<AccountInfo<'info>>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CompoundHash<'info> {
    #[account(mut, seeds = [b"global_state"], bump)]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut, seeds = [b"user_state", user.key().as_ref()], bump)]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct DrainVault<'info> {
    #[account(mut, seeds = [b"global_state"], bump, constraint = global_state.authority == authority.key())]
    pub global_state: Account<'info, GlobalState>,
    
    /// CHECK: Vault PDA
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ResetUser<'info> {
    #[account(mut, seeds = [b"global_state"], bump, constraint = global_state.authority == authority.key())]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut)]
    pub user_state: Account<'info, UserState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimEarnings<'info> {
    #[account(mut, seeds = [b"global_state"], bump)]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut, seeds = [b"user_state", user.key().as_ref()], bump)]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    /// CHECK: SOL Vault
    #[account(mut, seeds = [b"vault"], bump)]
    pub sol_vault: AccountInfo<'info>,
    
    #[account(mut)]
    pub gpu_vault: InterfaceAccount<'info, TokenAccount>,
    
    /// CHECK: GPU Vault Authority PDA
    #[account(seeds = [b"gpu_vault"], bump)]
    pub gpu_vault_authority: AccountInfo<'info>,
    
    #[account(mut)]
    pub user_gpu_account: InterfaceAccount<'info, TokenAccount>,
    
    #[account(mut)]
    pub dev_gpu_account: InterfaceAccount<'info, TokenAccount>,
    
    /// CHECK: Dev wallet
    #[account(mut, address = global_state.dev_wallet)]
    pub dev_wallet: AccountInfo<'info>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdatePrices<'info> {
    #[account(mut, seeds = [b"global_state"], bump, constraint = global_state.authority == authority.key())]
    pub global_state: Account<'info, GlobalState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetGpuToken<'info> {
    #[account(mut, seeds = [b"global_state"], bump, constraint = global_state.authority == authority.key())]
    pub global_state: Account<'info, GlobalState>,
    
    pub authority: Signer<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct GlobalState {
    pub authority: Pubkey,
    pub dev_wallet: Pubkey,
    pub total_mining_power: u64,
    pub total_unclaimed_sol: u64, // Track unclaimed SOL earnings
    pub total_unclaimed_gpu: u64, // Track unclaimed GPU earnings
    pub initialized: bool,
    pub daily_pool_percentage: u8, // % of TVL mineable per day
    pub base_buy_rate: u64, // MH/s per SOL at TVL=1
    pub protocol_fee_val: u8,
    pub gpu_penalty_bps: u16, // 15% = 1500 basis points
    pub sol_usd_price: u64, // SOL price in USD with 8 decimals
    pub gpu_usd_price: u64, // GPU price in USD with 8 decimals
    pub gpu_token_mint: Pubkey, // GPU token mint address (configurable)
}

#[account]
#[derive(InitSpace)]
pub struct UserState {
    pub owner: Pubkey,
    pub mining_power: u64,
    pub unclaimed_earnings: u64, // Track user's unclaimed SOL
    pub unclaimed_gpu_earnings: u64, // Track user's unclaimed GPU
    pub last_claim: i64,
    pub referrer: Option<Pubkey>,
    pub total_sol_claimed: u64, // Total SOL claimed all-time
    pub total_gpu_claimed: u64, // Total GPU tokens claimed all-time
}

#[error_code]
pub enum ErrorCode {
    #[msg("Not initialized")]
    NotInitialized,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid seed amount")]
    InvalidSeedAmount,
    #[msg("Overflow")]
    Overflow,
    #[msg("Division by zero")]
    DivisionByZero,
    #[msg("Self referral")]
    SelfReferral,
    #[msg("Insufficient funds")]
    InsufficientFunds,
    #[msg("Price not set")]
    PriceNotSet,
}
