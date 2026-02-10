use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};
use anchor_instruction_sysvar::Ed25519InstructionSignatures;
use solana_program::{sysvar::instructions::load_instruction_at_checked, ed25519_program, hash::hash};

use crate::{errors::DiceError, state::Bet};

pub const HOUSE_EDGE: u64 = 150; // 1.5% (bps)
pub const MIN_BET_LAMPORTS: u64 = 10_000_000; // 0.01 SOL
pub const MIN_ROLL: u8 = 2;
pub const MAX_ROLL: u8 = 96;

#[derive(Accounts)]
pub struct ResolveBet<'info>{
    #[account(mut)]
    pub house: Signer<'info>,

    /// CHECK: Player is validated by the bet account `has_one = player` constraint.
    #[account(mut)]
    pub player: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"vault", house.key().as_ref()],
        bump
    )]
    pub vault: SystemAccount<'info>,

    #[account(
        mut,
        has_one = player,
        close = player,
        seeds = [b"bet", vault.key().as_ref(), bet.seed.to_le_bytes().as_ref()],
        bump = bet.bump
    )]
    pub bet: Account<'info, Bet>,

    #[account(
        address = solana_program::sysvar::instructions::ID
    )]
    /// CHECK: Verified by address constraint to the instructions sysvar.
    pub instruction_sysvar: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> ResolveBet<'info>{

    pub fn verify_ed25519_signature(&mut self, sig: &[u8]) -> Result<()>{
        let ix = load_instruction_at_checked(
            0, 
            &self.instruction_sysvar.to_account_info()
        ).map_err(|_| DiceError::Ed25519Program)?;

        require_eq!(ix.program_id, ed25519_program::ID, DiceError::Ed25519Program);
        require_eq!(ix.accounts.len(), 0, DiceError::Ed25519Accounts);

        let signatures = Ed25519InstructionSignatures::unpack(&ix.data)
            .map_err(|_| DiceError::Ed25519Header)?;
        require_eq!(signatures.0.len(), 1, DiceError::Ed25519DataLength);

        let signature = &signatures.0[0];
        require!(signature.is_verifiable, DiceError::Ed25519Header);

        let public_key = signature.public_key.ok_or(DiceError::Ed25519Pubkey)?;
        require_eq!(public_key, self.house.key(), DiceError::Ed25519Pubkey);

        let message = signature.message.as_ref().ok_or(DiceError::Ed25519Message)?;
        let bet_bytes = self.bet.to_slice();
        require!(
            message.as_slice() == bet_bytes.as_slice(),
            DiceError::Ed25519Message
        );

        let sig_bytes: [u8; 64] = sig.try_into().map_err(|_| DiceError::Ed25519Signature)?;
        let on_chain_sig = signature.signature.ok_or(DiceError::Ed25519Signature)?;
        require!(on_chain_sig == sig_bytes, DiceError::Ed25519Signature);

        Ok(())
    }

    pub fn resolve_bet(&mut self, sig: &[u8], bumps: &ResolveBetBumps) -> Result<()> {
        require!(self.bet.amount >= MIN_BET_LAMPORTS, DiceError::MinimumBet);
        require!(self.bet.roll >= MIN_ROLL, DiceError::MinimumRoll);
        require!(self.bet.roll <= MAX_ROLL, DiceError::MaximumRoll);

        let payout = calculate_payout(self.bet.amount, self.bet.roll)?;
        require!(
            payout <= self.vault.to_account_info().lamports(),
            DiceError::MaximumBet
        );

        let hash_bytes = hash(sig).to_bytes();
        let roll_result = (u16::from_le_bytes([hash_bytes[0], hash_bytes[1]]) % 100) + 1;

        if roll_result as u8 <= self.bet.roll {
            let accounts = Transfer {
                from: self.vault.to_account_info(),
                to: self.player.to_account_info(),
            };

            let signer_seeds: &[&[&[u8]]] =
                &[&[b"vault", &self.house.key().to_bytes(), &[bumps.vault]]];

            let ctx = CpiContext::new_with_signer(
                self.system_program.to_account_info(),
                accounts,
                signer_seeds,
            );

            transfer(ctx, payout)?;
        }

        Ok(())
    }
}

fn calculate_payout(amount: u64, roll: u8) -> Result<u64> {
    let numerator = (amount as u128)
        .checked_mul((10_000 - HOUSE_EDGE) as u128)
        .ok_or(DiceError::Overflow)?;
    let denom = (roll as u128).checked_mul(100u128).ok_or(DiceError::Overflow)?;
    let payout = numerator.checked_div(denom).ok_or(DiceError::Overflow)?;
    Ok(payout as u64)
}
