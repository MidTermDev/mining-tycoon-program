use anchor_lang::prelude::*;

declare_id!("EfNnixKppGUq922Gzijt3mhDaKNAYsAFQ3BK9mtYGPU");

#[program]
pub mod bakedbeans_solana {
    use super::*;

    /// Initialize with new mining pool model
    pub fn initialize(ctx: Context<Initialize>, seed_amount: u64, dev_wallet: Pubkey) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        
        require!(seed_amount > 0, ErrorCode::InvalidSeedAmount);
        
        global_state.authority = ctx.accounts.authority.key();
        global_state.dev_wallet = dev_wallet;
        global_state.total_mining_power = 0;
        global_state.total_unclaimed_sol = 0;
        global_state.initialized = true;
        global_state.daily_pool_percentage = 10; // 10% of TVL per day
        global_state.base_buy_rate = 1000; // 1000 MH/s per SOL at TVL=1
        global_state.protocol_fee_val = 10;
        
        msg!("Mining Tycoon v2 initialized - Mining Pool Model");
        
        Ok(())
    }

    /// Buy MH/s - rate scales with TVL
    pub fn buy_mining_power(ctx: Context<BuyMiningPower>, amount: u64, referrer: Option<Pubkey>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        require!(amount > 0, ErrorCode::InvalidAmount);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        if user_state.referrer.is_none() && referrer.is_some() {
            let ref_key = referrer.unwrap();
            require!(ref_key != ctx.accounts.buyer.key(), ErrorCode::SelfReferral);
            user_state.referrer = Some(ref_key);
        }
        
        // Calculate MH/s based on TVL
        let vault_balance = ctx.accounts.vault.to_account_info().lamports();
        let mhs_bought = calculate_mhs_for_sol(
            amount,
            vault_balance,
            global_state.base_buy_rate
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
        
        msg!("Bought {} MH/s for {} lamports", mhs_after_fee, amount);
        
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

    /// Claim accumulated SOL earnings
    pub fn claim_earnings(ctx: Context<ClaimEarnings>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        require!(user_state.mining_power > 0, ErrorCode::InvalidAmount);
        
        // Calculate new earnings (excluding unclaimed from TVL)
        let vault_balance = ctx.accounts.vault.to_account_info().lamports();
        let mineable_tvl = vault_balance.checked_sub(global_state.total_unclaimed_sol).ok_or(ErrorCode::Overflow)?;
        
        let new_earnings = calculate_earnings(
            user_state.mining_power,
            global_state.total_mining_power,
            user_state.last_claim,
            clock.unix_timestamp,
            mineable_tvl,
            global_state.daily_pool_percentage
        )?;
        
        // Add to unclaimed
        user_state.unclaimed_earnings = user_state.unclaimed_earnings
            .checked_add(new_earnings)
            .ok_or(ErrorCode::Overflow)?;
        global_state.total_unclaimed_sol = global_state.total_unclaimed_sol
            .checked_add(new_earnings)
            .ok_or(ErrorCode::Overflow)?;
        user_state.last_claim = clock.unix_timestamp;
        
        // Claim all unclaimed
        let total_to_claim = user_state.unclaimed_earnings;
        require!(total_to_claim > 0, ErrorCode::InvalidAmount);
        require!(vault_balance >= total_to_claim, ErrorCode::InsufficientFunds);
        
        // Apply protocol fee
        let fee = total_to_claim.checked_mul(global_state.protocol_fee_val as u64)
            .ok_or(ErrorCode::Overflow)?
            .checked_div(100)
            .ok_or(ErrorCode::DivisionByZero)?;
        let payout = total_to_claim.checked_sub(fee).ok_or(ErrorCode::Overflow)?;
        
        // Reset unclaimed
        user_state.unclaimed_earnings = 0;
        global_state.total_unclaimed_sol = global_state.total_unclaimed_sol
            .checked_sub(total_to_claim)
            .ok_or(ErrorCode::Overflow)?;
        
        // Transfers
        let vault_bump = ctx.bumps.vault;
        let signer_seeds: &[&[&[u8]]] = &[&[b"vault", &[vault_bump]]];
        
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.user.to_account_info(),
                },
                signer_seeds,
            ),
            payout,
        )?;
        
        anchor_lang::system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.dev_wallet.to_account_info(),
                },
                signer_seeds,
            ),
            fee,
        )?;
        
        msg!("Claimed {} lamports (fee: {})", payout, fee);
        
        Ok(())
    }

    /// Initialize user account
    pub fn init_user(ctx: Context<InitUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        user_state.owner = ctx.accounts.user.key();
        user_state.mining_power = 0;
        user_state.unclaimed_earnings = 0;
        user_state.last_claim = clock.unix_timestamp;
        user_state.referrer = None;
        
        msg!("User initialized");
        
        Ok(())
    }

// Helper functions for new model
fn calculate_mhs_for_sol(sol_amount: u64, vault_balance: u64, base_rate: u64) -> Result<u64> {
    // Work with lamports to avoid integer division issues
    // MH/s = (lamports × base_rate × 100) / (100e9 + vault_lamports)
    
    let lamports = sol_amount as u128;
    let vault = vault_balance as u128;
    let rate = base_rate as u128;
    
    // numerator = lamports × base_rate × 100
    let numerator = lamports.checked_mul(rate).ok_or(ErrorCode::Overflow)?
        .checked_mul(100).ok_or(ErrorCode::Overflow)?;
    
    // denominator = 100e9 + vault_lamports
    let denominator = (100_000_000_000u128).checked_add(vault).ok_or(ErrorCode::Overflow)?;
    
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
    
    /// CHECK: Dev wallet
    #[account(mut, address = global_state.dev_wallet)]
    pub dev_wallet: AccountInfo<'info>,
    
    /// CHECK: Optional referrer
    #[account(mut)]
    pub referrer_state: Option<Account<'info, UserState>>,
    
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
    
    /// CHECK: Vault
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: AccountInfo<'info>,
    
    /// CHECK: Dev wallet
    #[account(mut, address = global_state.dev_wallet)]
    pub dev_wallet: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct GlobalState {
    pub authority: Pubkey,
    pub dev_wallet: Pubkey,
    pub total_mining_power: u64,
    pub total_unclaimed_sol: u64, // Track unclaimed earnings (not part of mineable TVL)
    pub initialized: bool,
    pub daily_pool_percentage: u8, // % of TVL mineable per day
    pub base_buy_rate: u64, // MH/s per SOL at TVL=1
    pub protocol_fee_val: u8,
}

#[account]
#[derive(InitSpace)]
pub struct UserState {
    pub owner: Pubkey,
    pub mining_power: u64,
    pub unclaimed_earnings: u64, // Track user's unclaimed SOL
    pub last_claim: i64,
    pub referrer: Option<Pubkey>,
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
}
