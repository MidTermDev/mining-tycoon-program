use anchor_lang::prelude::*;

declare_id!("vfkjARttLMEns3qrxW58J8MiDXcTwcXzH8qZaYAVGPU");

#[program]
pub mod bakedbeans_solana {
    use super::*;

    /// Initialize the global state (admin only, one-time setup)
    pub fn initialize(ctx: Context<Initialize>, seed_amount: u64, dev_wallet: Pubkey) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        
        require!(seed_amount > 0, ErrorCode::InvalidSeedAmount);
        
        global_state.authority = ctx.accounts.authority.key();
        global_state.dev_wallet = dev_wallet;
        global_state.market_gpus = 108_000_000_000; // 108B GPUs (Original model)
        global_state.initialized = true;
        global_state.hashpower_to_hire_1miner = 1_080_000; // 1.08M hash = 1 MH/s (12.5 days)
        global_state.protocol_fee_val = 10;
        global_state.psn = 5_000;
        global_state.psnh = 10_000;
        
        msg!("Mining Tycoon initialized with market GPUs: {}", global_state.market_gpus);
        
        Ok(())
    }

    /// Buy MH/s with SOL
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
        
        let current_vault = ctx.accounts.vault.to_account_info().lamports();
        let virtual_vault = current_vault
            .checked_add(100_000_000_000)
            .ok_or(ErrorCode::Overflow)?;
        
        let hashpower_bought = calculate_trade(
            amount,
            virtual_vault,
            global_state.market_gpus,
            global_state.psn,
            global_state.psnh,
        )?;
        
        let fee = protocol_fee(hashpower_bought, global_state.protocol_fee_val)?;
        let hashpower_after_fee = hashpower_bought.checked_sub(fee).ok_or(ErrorCode::Overflow)?;
        
        let new_miners = hashpower_after_fee
            .checked_div(global_state.hashpower_to_hire_1miner)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        user_state.mining_power = user_state.mining_power
            .checked_add(new_miners)
            .ok_or(ErrorCode::Overflow)?;
        user_state.accumulated_hashpower = 0;
        user_state.last_compound = clock.unix_timestamp;
        
        if let Some(referrer_state) = ctx.accounts.referrer_state.as_mut() {
            let referral_bonus = hashpower_after_fee.checked_div(20).ok_or(ErrorCode::DivisionByZero)?;
            referrer_state.accumulated_hashpower = referrer_state.accumulated_hashpower
                .checked_add(referral_bonus)
                .ok_or(ErrorCode::Overflow)?;
            msg!("Sent {} hashpower to referrer", referral_bonus);
        }
        
        msg!("Bought {} MH/s for {} lamports", new_miners, amount);
        
        Ok(())
    }

    /// Compound hashpower to get more MH/s
    pub fn compound_hashpower(ctx: Context<CompoundHashpower>, referrer: Option<Pubkey>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        if user_state.referrer.is_none() && referrer.is_some() {
            let ref_key = referrer.unwrap();
            require!(ref_key != ctx.accounts.user.key(), ErrorCode::SelfReferral);
            user_state.referrer = Some(ref_key);
        }
        
        let hashpower_used = get_accumulated_hashpower(user_state, global_state.hashpower_to_hire_1miner, clock.unix_timestamp)?;
        
        let new_miners = hashpower_used
            .checked_div(global_state.hashpower_to_hire_1miner)
            .ok_or(ErrorCode::DivisionByZero)?;
        
        user_state.mining_power = user_state.mining_power
            .checked_add(new_miners)
            .ok_or(ErrorCode::Overflow)?;
        user_state.accumulated_hashpower = 0;
        user_state.last_compound = clock.unix_timestamp;
        
        if let Some(referrer_state) = ctx.accounts.referrer_state.as_mut() {
            let referral_bonus = hashpower_used.checked_div(20).ok_or(ErrorCode::DivisionByZero)?;
            referrer_state.accumulated_hashpower = referrer_state.accumulated_hashpower
                .checked_add(referral_bonus)
                .ok_or(ErrorCode::Overflow)?;
            msg!("Sent {} hashpower to referrer", referral_bonus);
        }
        
        let market_boost = hashpower_used.checked_div(5).ok_or(ErrorCode::DivisionByZero)?;
        global_state.market_gpus = global_state.market_gpus
            .checked_add(market_boost)
            .ok_or(ErrorCode::Overflow)?;
        
        msg!("Compounded {} hashpower into {} MH/s", hashpower_used, new_miners);
        
        Ok(())
    }

    /// Sell hashpower for SOL
    pub fn sell_hashpower(ctx: Context<SellHashpower>) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(global_state.initialized, ErrorCode::NotInitialized);
        
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        let has_hashpower = get_accumulated_hashpower(user_state, global_state.hashpower_to_hire_1miner, clock.unix_timestamp)?;
        require!(has_hashpower > 0, ErrorCode::InvalidAmount);
        
        let hashpower_value = calculate_trade(
            has_hashpower,
            global_state.market_gpus,
            ctx.accounts.vault.to_account_info().lamports(),
            global_state.psn,
            global_state.psnh,
        )?;
        
        let fee = protocol_fee(hashpower_value, global_state.protocol_fee_val)?;
        let payout = hashpower_value.checked_sub(fee).ok_or(ErrorCode::Overflow)?;
        
        require!(payout > 0, ErrorCode::InvalidAmount);
        require!(ctx.accounts.vault.to_account_info().lamports() >= hashpower_value, ErrorCode::InsufficientFunds);
        
        user_state.accumulated_hashpower = 0;
        user_state.last_compound = clock.unix_timestamp;
        
        global_state.market_gpus = global_state.market_gpus
            .checked_add(has_hashpower)
            .ok_or(ErrorCode::Overflow)?;
        
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
        
        msg!("Sold {} hashpower for {} lamports (fee: {})", has_hashpower, payout, fee);
        
        Ok(())
    }

    /// Initialize user state account
    pub fn init_user(ctx: Context<InitUser>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        let clock = Clock::get()?;
        
        user_state.owner = ctx.accounts.user.key();
        user_state.mining_power = 0;
        user_state.accumulated_hashpower = 0;
        user_state.last_compound = clock.unix_timestamp;
        user_state.referrer = None;
        
        msg!("User state initialized for {}", ctx.accounts.user.key());
        
        Ok(())
    }

    /// Admin: Update hashpower_to_hire_1miner
    pub fn update_hashpower_requirement(ctx: Context<UpdateConfig>, new_value: u64) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(new_value > 0, ErrorCode::InvalidAmount);
        
        global_state.hashpower_to_hire_1miner = new_value;
        msg!("Updated hashpower_to_hire_1miner to {}", new_value);
        
        Ok(())
    }

    /// Admin: Update market_gpus
    pub fn update_market_gpus(ctx: Context<UpdateConfig>, new_value: u64) -> Result<()> {
        let global_state = &mut ctx.accounts.global_state;
        require!(new_value > 0, ErrorCode::InvalidAmount);
        
        global_state.market_gpus = new_value;
        msg!("Updated market_gpus to {}", new_value);
        
        Ok(())
    }

    /// Admin: Multiply user's mining power by 10x
    pub fn multiply_user_mining_power(ctx: Context<MultiplyMiningPower>) -> Result<()> {
        let user_state = &mut ctx.accounts.user_state;
        
        let current = user_state.mining_power;
        let new_amount = current.checked_mul(10).ok_or(ErrorCode::Overflow)?;
        
        user_state.mining_power = new_amount;
        msg!("Multiplied user {} MH/s from {} to {}", 
            user_state.owner, current, new_amount);
        
        Ok(())
    }
}

// Helper functions
fn calculate_trade(rt: u64, rs: u64, bs: u64, psn: u64, psnh: u64) -> Result<u64> {
    let rt_u128 = rt as u128;
    let rs_u128 = rs as u128;
    let bs_u128 = bs as u128;
    let psn_u128 = psn as u128;
    let psnh_u128 = psnh as u128;
    
    let numerator = rt_u128
        .checked_mul(bs_u128)
        .ok_or(ErrorCode::Overflow)?
        .checked_mul(psn_u128)
        .ok_or(ErrorCode::Overflow)?;
    
    let denominator = rs_u128
        .checked_mul(psnh_u128)
        .ok_or(ErrorCode::Overflow)?;
    
    let result = numerator
        .checked_div(denominator)
        .ok_or(ErrorCode::DivisionByZero)?;
    
    u64::try_from(result).map_err(|_| ErrorCode::Overflow.into())
}

fn protocol_fee(amount: u64, fee_val: u8) -> Result<u64> {
    amount
        .checked_mul(fee_val as u64)
        .ok_or(ErrorCode::Overflow)?
        .checked_div(100)
        .ok_or(ErrorCode::DivisionByZero.into())
}

fn get_accumulated_hashpower(user_state: &UserState, hashpower_to_hire: u64, current_time: i64) -> Result<u64> {
    let hashpower_since_compound = get_hashpower_since_last_compound(user_state, hashpower_to_hire, current_time)?;
    user_state.accumulated_hashpower
        .checked_add(hashpower_since_compound)
        .ok_or(ErrorCode::Overflow.into())
}

fn get_hashpower_since_last_compound(user_state: &UserState, hashpower_to_hire: u64, current_time: i64) -> Result<u64> {
    let time_passed = (current_time - user_state.last_compound) as u64;
    let seconds_passed = std::cmp::min(hashpower_to_hire, time_passed);
    
    seconds_passed
        .checked_mul(user_state.mining_power)
        .ok_or(ErrorCode::Overflow.into())
}

// Account structures
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
    
    /// CHECK: Vault account for holding SOL
    #[account(
        mut,
        seeds = [b"vault"],
        bump
    )]
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
    #[account(
        mut,
        seeds = [b"global_state"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(
        mut,
        seeds = [b"user_state", buyer.key().as_ref()],
        bump
    )]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    /// CHECK: Vault account for holding SOL
    #[account(
        mut,
        seeds = [b"vault"],
        bump
    )]
    pub vault: AccountInfo<'info>,
    
    /// CHECK: Dev wallet to receive fees
    #[account(
        mut,
        address = global_state.dev_wallet
    )]
    pub dev_wallet: AccountInfo<'info>,
    
    /// CHECK: Optional referrer state account
    #[account(mut)]
    pub referrer_state: Option<Account<'info, UserState>>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CompoundHashpower<'info> {
    #[account(
        mut,
        seeds = [b"global_state"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(
        mut,
        seeds = [b"user_state", user.key().as_ref()],
        bump
    )]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    /// CHECK: Optional referrer state account
    #[account(mut)]
    pub referrer_state: Option<Account<'info, UserState>>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"global_state"],
        bump,
        constraint = global_state.authority == authority.key()
    )]
    pub global_state: Account<'info, GlobalState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct MultiplyMiningPower<'info> {
    #[account(
        mut,
        seeds = [b"global_state"],
        bump,
        constraint = global_state.authority == authority.key()
    )]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(mut)]
    pub user_state: Account<'info, UserState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SellHashpower<'info> {
    #[account(
        mut,
        seeds = [b"global_state"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,
    
    #[account(
        mut,
        seeds = [b"user_state", user.key().as_ref()],
        bump,
        constraint = user_state.owner == user.key()
    )]
    pub user_state: Account<'info, UserState>,
    
    #[account(mut)]
    pub user: Signer<'info>,
    
    /// CHECK: Vault account for holding SOL
    #[account(
        mut,
        seeds = [b"vault"],
        bump
    )]
    pub vault: AccountInfo<'info>,
    
    /// CHECK: Dev wallet to receive fees
    #[account(
        mut,
        address = global_state.dev_wallet
    )]
    pub dev_wallet: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

// State accounts
#[account]
#[derive(InitSpace)]
pub struct GlobalState {
    pub authority: Pubkey,
    pub dev_wallet: Pubkey,
    pub market_gpus: u64,
    pub initialized: bool,
    pub hashpower_to_hire_1miner: u64,
    pub protocol_fee_val: u8,
    pub psn: u64,
    pub psnh: u64,
}

#[account]
#[derive(InitSpace)]
pub struct UserState {
    pub owner: Pubkey,
    pub mining_power: u64,
    pub accumulated_hashpower: u64,
    pub last_compound: i64,
    pub referrer: Option<Pubkey>,
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("Contract not initialized")]
    NotInitialized,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid seed amount")]
    InvalidSeedAmount,
    #[msg("Overflow occurred")]
    Overflow,
    #[msg("Division by zero")]
    DivisionByZero,
    #[msg("Cannot refer yourself")]
    SelfReferral,
    #[msg("Insufficient funds in vault")]
    InsufficientFunds,
}
