use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{create_metadata_accounts_v3, CreateMetadataAccountsV3, Metadata},
    token::{mint_to, Mint, MintTo, Token, TokenAccount},
};
use gateway::{Gateway, OutboundMessage};
use mpl_token_metadata::{
    pda::{find_master_edition_account, find_metadata_account},
    state::{DataV2, TokenMetadataAccount},
};

declare_id!("UnivNFT1111111111111111111111111111111111");

#[program]
pub mod universal_nft {
    use super::*;

    /// Initialize the universal NFT program
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let nft_program = &mut ctx.accounts.nft_program;
        nft_program.authority = ctx.accounts.authority.key();
        nft_program.total_supply = 0;
        nft_program.gateway = ctx.accounts.gateway.key();
        
        msg!("Universal NFT program initialized");
        Ok(())
    }

    /// Mint a new NFT (can be called locally or via cross-chain)
    pub fn mint_nft(
        ctx: Context<MintNft>,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        // Mint the token
        mint_to(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.token_account.to_account_info(),
                    authority: ctx.accounts.mint_authority.to_account_info(),
                },
            ),
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
                    mint_authority: ctx.accounts.mint_authority.to_account_info(),
                    update_authority: ctx.accounts.mint_authority.to_account_info(),
                    payer: ctx.accounts.payer.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    rent: ctx.accounts.rent.to_account_info(),
                },
            ),
            data_v2,
            false, // is_mutable
            true,  // update_authority_is_signer
            None,  // collection_details
        )?;

        // Update program state
        let nft_program = &mut ctx.accounts.nft_program;
        nft_program.total_supply = nft_program.total_supply.checked_add(1).unwrap();

        msg!("NFT minted: {} - {}", name, uri);
        Ok(())
    }

    /// Initiate cross-chain transfer to ZetaChain
    pub fn transfer_to_zetachain(
        ctx: Context<TransferToZetachain>,
        destination_chain_id: u64,
        recipient: [u8; 32],
        token_id: u64,
    ) -> Result<()> {
        // Burn the NFT on Solana (transfer to program authority)
        // In a real implementation, you'd want to lock it instead of burning
        
        // Create cross-chain message
        let message = CrossChainMessage {
            message_type: MessageType::Transfer,
            token_id,
            recipient,
            metadata_uri: ctx.accounts.nft_metadata.uri.clone(),
            name: ctx.accounts.nft_metadata.name.clone(),
            symbol: ctx.accounts.nft_metadata.symbol.clone(),
        };

        // Send message via gateway
        let message_bytes = message.try_to_vec()?;
        
        // Call gateway to send cross-chain message
        gateway::cpi::call(
            CpiContext::new(
                ctx.accounts.gateway_program.to_account_info(),
                gateway::cpi::accounts::Call {
                    signer: ctx.accounts.authority.to_account_info(),
                    pda: ctx.accounts.gateway_pda.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                },
            ),
            destination_chain_id,
            message_bytes,
        )?;

        msg!("Cross-chain transfer initiated for token {}", token_id);
        Ok(())
    }

    /// Handle incoming cross-chain message from ZetaChain
    pub fn handle_cross_chain_call(
        ctx: Context<HandleCrossChainCall>,
        sender: [u8; 32],
        source_chain_id: u64,
        message: Vec<u8>,
    ) -> Result<()> {
        // Parse the incoming message
        let cross_chain_message: CrossChainMessage = CrossChainMessage::try_from_slice(&message)?;
        
        match cross_chain_message.message_type {
            MessageType::Transfer => {
                // Mint NFT to recipient on Solana
                self::mint_nft(
                    Context::new(
                        ctx.program_id,
                        &mut ctx.accounts.mint_accounts,
                        ctx.remaining_accounts,
                        ctx.bumps.clone(),
                    ),
                    cross_chain_message.name,
                    cross_chain_message.symbol,
                    cross_chain_message.metadata_uri,
                )?;
                
                msg!("Cross-chain NFT transfer completed");
            }
        }
        
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
    
    /// CHECK: Gateway program account
    pub gateway: AccountInfo<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintNft<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump
    )]
    pub nft_program: Account<'info, NftProgramState>,

    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = mint_authority,
    )]
    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = recipient,
    )]
    pub token_account: Account<'info, TokenAccount>,

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
    pub metadata: AccountInfo<'info>,

    pub mint_authority: Signer<'info>,
    pub recipient: SystemAccount<'info>,

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
        bump
    )]
    pub nft_program: Account<'info, NftProgramState>,

    pub authority: Signer<'info>,
    
    /// CHECK: NFT metadata account
    pub nft_metadata: AccountInfo<'info>,
    
    /// CHECK: Gateway PDA
    pub gateway_pda: AccountInfo<'info>,
    
    /// CHECK: Gateway program
    pub gateway_program: Program<'info, Gateway>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct HandleCrossChainCall<'info> {
    #[account(
        mut,
        seeds = [b"nft-program"],
        bump
    )]
    pub nft_program: Account<'info, NftProgramState>,
    
    /// Accounts needed for minting (will be provided dynamically)
    pub mint_accounts: MintNft<'info>,
}

// Program state
#[account]
#[derive(InitSpace)]
pub struct NftProgramState {
    pub authority: Pubkey,
    pub gateway: Pubkey,
    pub total_supply: u64,
}

// Cross-chain message structure
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CrossChainMessage {
    pub message_type: MessageType,
    pub token_id: u64,
    pub recipient: [u8; 32],
    #[max_len(200)]
    pub metadata_uri: String,
    #[max_len(32)]
    pub name: String,
    #[max_len(10)]
    pub symbol: String,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum MessageType {
    Transfer,
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
}