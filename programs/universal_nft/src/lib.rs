use anchor_lang::prelude::*;
use anchor_lang::solana_program::compute_budget::ComputeBudgetInstruction;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{create_metadata_accounts_v3, CreateMetadataAccountsV3, Metadata},
    token::{mint_to, transfer, Mint, MintTo, Token, TokenAccount, Transfer},
};
use mpl_token_metadata::{
    pda::{find_metadata_account},
    state::{DataV2, Metadata as TokenMetadata},
};

declare_id!("7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsUgit");

#[program]
pub mod universal_nft {
    use super::*;

    /// Initialize the universal NFT program
    pub fn initialize(ctx: Context<Initialize>, gateway: Pubkey) -> Result<()> {
        let nft_program = &mut ctx.accounts.nft_program;
        nft_program.authority = ctx.accounts.authority.key();
        nft_program.total_supply = 0;
        nft_program.gateway = gateway;
        nft_program.nonce = 0;
        nft_program.bump = ctx.bumps.nft_program;
        
        msg!("Universal NFT program initialized with gateway: {}", gateway);
        Ok(())
    }

    /// Mint a new NFT (can be called locally or via cross-chain)
    pub fn mint_nft(
        ctx: Context<MintNft>,
        name: String,
        symbol: String,
        uri: String,
        recipient: Pubkey,
    ) -> Result<()> {
        // Validate inputs
        require!(name.len() <= 32, NftError::InvalidMetadata);
        require!(symbol.len() <= 10, NftError::InvalidMetadata);
        require!(uri.len() <= 200, NftError::InvalidMetadata);

        // Mint the token
        mint_to(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.nft_program.to_account_info(),
                },
            ).with_signer(&[&[
                b"nft-program",
                &[ctx.accounts.nft_program.bump]
            ]]),
            1, // NFTs have supply of 1
        )?;

        // Create metadata
        let data_v2 = DataV2 {
            name: name.clone(),
            symbol: symbol.clone(),
            uri: uri.clone(),
            seller_fee_basis_points: 0,
            creators: None,
            collection: None,
            uses: None,
        };

        create_metadata_accounts_v3(
            CpiContext::new(
                ctx.accounts.token_metadata_program.to_account_info(),
                CreateMetadataAccountsV3 {
                    metadata: ctx.accounts.metadata.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    mint_authority: ctx.accounts.nft_program.to_account_info(),
                    update_authority: ctx.accounts.nft_program.to_account_info(),
                    payer: ctx.accounts.payer.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                },
            ).with_signer(&[&[
                b"nft-program",
                &[ctx.accounts.nft_program.bump]
            ]]),
            data_v2,
            false, // is_mutable
            true,  // update_authority_is_signer
            None,  // collection_details
        )?;

        // Update program state
        let nft_program = &mut ctx.accounts.nft_program;
        nft_program.total_supply = nft_program.total_supply
            .checked_add(1)
            .ok_or(NftError::Overflow)?;

        // Store NFT info for cross-chain operations
        let nft_info = &mut ctx.accounts.nft_info;
        nft_info.mint = ctx.accounts.mint.key();
        nft_info.owner = recipient;
        nft_info.metadata_uri = uri.clone();
        nft_info.name = name.clone();
        nft_info.symbol = symbol.clone();
        nft_info.is_locked = false;
        nft_info.bump = ctx.bumps.nft_info;

        msg!("NFT minted: {} - {} to {}", name, uri, recipient);
        Ok(())
    }

    /// Initiate cross-chain transfer to ZetaChain
    pub fn transfer_to_zetachain(
        ctx: Context<TransferToZetachain>,
        destination_chain_id: u64,
        recipient: [u8; 32],
        nonce: u64,
    ) -> Result<()> {
        let nft_info = &mut ctx.accounts.nft_info;
        let nft_program = &mut ctx.accounts.nft_program;

        // Security checks
        require!(nft_info.owner == ctx.accounts.owner.key(), NftError::Unauthorized);
        require!(!nft_info.is_locked, NftError::TokenLocked);
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);

        // Lock the NFT (don't burn, just transfer to program)
        transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.owner_token_account.to_account_info(),
                    to: ctx.accounts.program_token_account.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                },
            ),
            1,
        )?;

        // Update NFT state
        nft_info.is_locked = true;
        nft_info.cross_chain_recipient = recipient;
        nft_program.nonce = nonce;

        // Create cross-chain message
        let message = CrossChainMessage {
            message_type: MessageType::Transfer,
            mint: nft_info.mint,
            recipient,
            metadata_uri: nft_info.metadata_uri.clone(),
            name: nft_info.name.clone(),
            symbol: nft_info.symbol.clone(),
            nonce,
        };

        // Serialize message
        let message_bytes = message.try_to_vec()?;
        
        // Send via gateway (placeholder - would use actual gateway CPI)
        msg!("Cross-chain transfer initiated for mint {} to chain {} recipient {:?}", 
            nft_info.mint, destination_chain_id, recipient);
        msg!("Message: {:?}", message_bytes);

        Ok(())
    }

    /// Handle incoming cross-chain message from ZetaChain
    pub fn handle_cross_chain_call(
        ctx: Context<HandleCrossChainCall>,
        sender: [u8; 32],
        source_chain_id: u64,
        message: Vec<u8>,
        nonce: u64,
    ) -> Result<()> {
        let nft_program = &mut ctx.accounts.nft_program;
        
        // Replay protection
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);
        nft_program.nonce = nonce;

        // Parse the incoming message
        let cross_chain_message: CrossChainMessage = 
            CrossChainMessage::try_from_slice(&message)
                .map_err(|_| NftError::InvalidMessage)?;
        
        match cross_chain_message.message_type {
            MessageType::Transfer => {
                // Validate recipient
                let recipient_pubkey = Pubkey::try_from(cross_chain_message.recipient)
                    .map_err(|_| NftError::InvalidRecipient)?;
                
                msg!("Handling cross-chain NFT transfer from chain {} to {}", 
                    source_chain_id, recipient_pubkey);
                
                // In a real implementation, you would mint the NFT here
                // This would require dynamic account creation
                msg!("Would mint NFT: {}", cross_chain_message.name);
            }
            MessageType::Unlock => {
                // Handle unlock for return transfers
                msg!("Handling NFT unlock for mint {}", cross_chain_message.mint);
            }
        }
        
        Ok(())
    }

    /// Unlock NFT after successful cross-chain return
    pub fn unlock_nft(ctx: Context<UnlockNft>, nonce: u64) -> Result<()> {
        let nft_info = &mut ctx.accounts.nft_info;
        let nft_program = &mut ctx.accounts.nft_program;

        // Security checks
        require!(nft_info.is_locked, NftError::TokenNotLocked);
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);
        
        // Transfer back to owner
        transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.program_token_account.to_account_info(),
                    to: ctx.accounts.owner_token_account.to_account_info(),
                    authority: ctx.accounts.nft_program.to_account_info(),
                },
            ).with_signer(&[&[
                b"nft-program",
                &[nft_program.bump]
            ]]),
            1,
        )?;

        // Update state
        nft_info.is_locked = false;
        nft_program.nonce = nonce;

        msg!("NFT unlocked for mint {}", nft_info.mint);
        Ok(())
    }
}

// Account structures
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + NftProgramState::INIT_SPACE,
        seeds = [b"nft-program"],
        bump
    )]
    pub nft_program: Account<'info, NftProgramState>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(name: String, symbol: String, uri: String, recipient: Pubkey)]
pub struct MintNft<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump = nft_program.bump
    )]
    pub nft_program: Account<'info, NftProgramState>,

    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = nft_program,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient,
    )]
    pub token_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = payer,
        space = 8 + NftInfo::INIT_SPACE,
        seeds = [b"nft-info", mint.key().as_ref()],
        bump
    )]
    pub nft_info: Account<'info, NftInfo>,

    /// CHECK: Metadata account
    #[account(
        mut,
        seeds = [
            b"metadata",
            token_metadata_program.key().as_ref(),
            mint.key().as_ref(),
        ],
        bump,
        seeds::program = token_metadata_program.key()
    )]
    pub metadata: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub rent: Sysvar<'info, Rent>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_metadata_program: Program<'info, Metadata>,
}

#[derive(Accounts)]
pub struct TransferToZetachain<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump = nft_program.bump
    )]
    pub nft_program: Account<'info, NftProgramState>,

    #[account(
        mut,
        seeds = [b"nft-info", nft_info.mint.as_ref()],
        bump = nft_info.bump,
        constraint = nft_info.owner == owner.key()
    )]
    pub nft_info: Account<'info, NftInfo>,

    pub owner: Signer<'info>,
    
    #[account(
        mut,
        associated_token::mint = nft_info.mint,
        associated_token::authority = owner,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = nft_info.mint,
        associated_token::authority = nft_program,
    )]
    pub program_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct HandleCrossChainCall<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump = nft_program.bump
    )]
    pub nft_program: Account<'info, NftProgramState>,
}

#[derive(Accounts)]
pub struct UnlockNft<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump = nft_program.bump
    )]
    pub nft_program: Account<'info, NftProgramState>,

    #[account(
        mut,
        seeds = [b"nft-info", nft_info.mint.as_ref()],
        bump = nft_info.bump,
        constraint = nft_info.owner == owner.key()
    )]
    pub nft_info: Account<'info, NftInfo>,

    pub owner: Signer<'info>,
    
    #[account(
        mut,
        associated_token::mint = nft_info.mint,
        associated_token::authority = owner,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = nft_info.mint,
        associated_token::authority = nft_program,
    )]
    pub program_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

// Program state
#[account]
#[derive(InitSpace)]
pub struct NftProgramState {
    pub authority: Pubkey,
    pub gateway: Pubkey,
    pub total_supply: u64,
    pub nonce: u64, // For replay protection
    pub bump: u8,
}

// NFT tracking info
#[account]
#[derive(InitSpace)]
pub struct NftInfo {
    pub mint: Pubkey,
    pub owner: Pubkey,
    #[max_len(200)]
    pub metadata_uri: String,
    #[max_len(32)]
    pub name: String,
    #[max_len(10)]
    pub symbol: String,
    pub is_locked: bool,
    pub cross_chain_recipient: [u8; 32],
    pub bump: u8,
}

// Cross-chain message structure
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct CrossChainMessage {
    pub message_type: MessageType,
    pub mint: Pubkey,
    pub recipient: [u8; 32],
    #[max_len(200)]
    pub metadata_uri: String,
    #[max_len(32)]
    pub name: String,
    #[max_len(10)]
    pub symbol: String,
    pub nonce: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub enum MessageType {
    Transfer,
    Unlock,
}

// Error types
#[error_code]
pub enum NftError {
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Invalid cross-chain message")]
    InvalidMessage,
    #[msg("Token not found")]
    TokenNotFound,
    #[msg("Token is locked")]
    TokenLocked,
    #[msg("Token is not locked")]
    TokenNotLocked,
    #[msg("Invalid nonce")]
    InvalidNonce,
    #[msg("Invalid recipient")]
    InvalidRecipient,
    #[msg("Invalid metadata")]
    InvalidMetadata,
    #[msg("Arithmetic overflow")]
    Overflow,
}