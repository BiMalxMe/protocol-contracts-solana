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

// this is the program id, dont forget to update if u redeploy
declare_id!("7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsUgit");

#[program]
pub mod universal_nft {
    use super::*;

    /// initilize the universal nft program, must be called once at start
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

    /// mint a new nft, can be called localy or from crosschain
    pub fn mint_nft(
        ctx: Context<MintNft>,
        name: String,
        symbol: String,
        uri: String,
        recipient: Pubkey,
    ) -> Result<()> {
        // check the input lengths so we dont break stuff
        require!(name.len() <= 32, NftError::InvalidMetadata);
        require!(symbol.len() <= 10, NftError::InvalidMetadata);
        require!(uri.len() <= 200, NftError::InvalidMetadata);

        // mint the token, only 1 for nft
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
            1, // nfts always have supply 1
        )?;

        // make the metadata for the nft
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
            false, // not mutable
            true,  // update authority is signer
            None,  // no collection details
        )?;

        // update the program state, add 1 to supply
        let nft_program = &mut ctx.accounts.nft_program;
        nft_program.total_supply = nft_program.total_supply
            .checked_add(1)
            .ok_or(NftError::Overflow)?;

        // save nft info for crosschain stuff
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

    /// start a crosschain transfer to zetachain, locks the nft
    pub fn transfer_to_zetachain(
        ctx: Context<TransferToZetachain>,
        destination_chain_id: u64,
        recipient: [u8; 32],
        nonce: u64,
    ) -> Result<()> {
        let nft_info = &mut ctx.accounts.nft_info;
        let nft_program = &mut ctx.accounts.nft_program;

        // do some security checks so only owner can transfer and not locked
        require!(nft_info.owner == ctx.accounts.owner.key(), NftError::Unauthorized);
        require!(!nft_info.is_locked, NftError::TokenLocked);
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);

        // lock the nft by moving it to program, dont burn it
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

        // update nft state to locked and set crosschain recipient
        nft_info.is_locked = true;
        nft_info.cross_chain_recipient = recipient;
        nft_program.nonce = nonce;

        // make the crosschain message
        let message = CrossChainMessage {
            message_type: MessageType::Transfer,
            mint: nft_info.mint,
            recipient,
            metadata_uri: nft_info.metadata_uri.clone(),
            name: nft_info.name.clone(),
            symbol: nft_info.symbol.clone(),
            nonce,
        };

        // serialize the message for sending
        let message_bytes = message.try_to_vec()?;
        
        // send via gateway (not implemented, just log for now)
        msg!("Cross-chain transfer initiated for mint {} to chain {} recipient {:?}", 
            nft_info.mint, destination_chain_id, recipient);
        msg!("Message: {:?}", message_bytes);

        Ok(())
    }

    /// handle incoming crosschain message from zetachain, like mint or unlock
    pub fn handle_cross_chain_call(
        ctx: Context<HandleCrossChainCall>,
        sender: [u8; 32],
        source_chain_id: u64,
        message: Vec<u8>,
        nonce: u64,
    ) -> Result<()> {
        let nft_program = &mut ctx.accounts.nft_program;
        
        // replay protection so we dont process same message twice
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);
        nft_program.nonce = nonce;

        // try to parse the incoming message, fail if not valid
        let cross_chain_message: CrossChainMessage = 
            CrossChainMessage::try_from_slice(&message)
                .map_err(|_| NftError::InvalidMessage)?;
        
        match cross_chain_message.message_type {
            MessageType::Transfer => {
                // check the recipient is valid pubkey
                let recipient_pubkey = Pubkey::try_from(cross_chain_message.recipient)
                    .map_err(|_| NftError::InvalidRecipient)?;
                
                msg!("Handling cross-chain NFT transfer from chain {} to {}", 
                    source_chain_id, recipient_pubkey);
                
                // here we would mint the nft, but not implemented yet
                // would need to create accounts on the fly
                msg!("Would mint NFT: {}", cross_chain_message.name);
            }
            MessageType::Unlock => {
                // handle unlock for return transfers
                msg!("Handling NFT unlock for mint {}", cross_chain_message.mint);
            }
        }
        
        Ok(())
    }

    /// unlock nft after it comes back from crosschain, send to owner
    pub fn unlock_nft(ctx: Context<UnlockNft>, nonce: u64) -> Result<()> {
        let nft_info = &mut ctx.accounts.nft_info;
        let nft_program = &mut ctx.accounts.nft_program;

        // check if locked and nonce is ok
        require!(nft_info.is_locked, NftError::TokenNotLocked);
        require!(nonce > nft_program.nonce, NftError::InvalidNonce);
        
        // move nft back to owner
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

        // update state to unlocked and set new nonce
        nft_info.is_locked = false;
        nft_program.nonce = nonce;

        msg!("NFT unlocked for mint {}", nft_info.mint);
        Ok(())
    }
}

// account structs for all the instructions, dont mess with the order
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

    /// check: this is the metadata account, dont use directly
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

// program state, stores main info for the contract
#[account]
#[derive(InitSpace)]
pub struct NftProgramState {
    pub authority: Pubkey,
    pub gateway: Pubkey,
    pub total_supply: u64,
    pub nonce: u64, // for replay protection, dont let it repeat
    pub bump: u8,
}

// nft tracking info, stores all the data for each nft
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

// crosschain message struct, used for sending nft data between chains
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

// error types for the program, try to keep them clear
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