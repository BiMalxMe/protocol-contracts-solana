# universal nft program for zetachain

cross-chain nft transfers between solana and zetachain/evm chains using lock/unlock mechanism.

## core features

- **cross-chain nft transfers** - solana ↔ zetachain ↔ evm
- **lock/unlock mechanism** - preserves original nfts (no burn/mint)
- **zetachain gateway integration** - direct protocol-contracts-solana usage
- **replay protection** - nonce-based security
- **solana optimized** - handles compute budget, rent, signers

## architecture

```
solana nft → lock → zetachain gateway → evm chain
solana nft ← unlock ← zetachain gateway ← evm chain
```

## key instructions

### mint_nft
```rust
pub fn mint_nft(name: String, symbol: String, uri: String, recipient: Pubkey)
```
creates spl token + metaplex metadata

### transfer_to_zetachain  
```rust
pub fn transfer_to_zetachain(destination_chain_id: u64, recipient: [u8; 32], nonce: u64)
```
locks nft on solana, sends cross-chain message via gateway

### handle_cross_chain_call
```rust  
pub fn handle_cross_chain_call(sender: [u8; 32], source_chain_id: u64, message: Vec<u8>, nonce: u64)
```
receives incoming messages from zetachain gateway

### unlock_nft
```rust
pub fn unlock_nft(nonce: u64) 
```
returns locked nft to original owner

## security features

```rust
// replay protection
require!(nonce > nft_program.nonce, NftError::InvalidNonce);

// ownership verification  
require!(nft_info.owner == ctx.accounts.owner.key(), NftError::Unauthorized);

// lock state management
require!(!nft_info.is_locked, NftError::TokenLocked);
```

## cross-chain message format

```rust
pub struct CrossChainMessage {
    pub message_type: MessageType,
    pub mint: Pubkey,
    pub recipient: [u8; 32],    // evm compatible
    pub metadata_uri: String,
    pub name: String, 
    pub symbol: String,
    pub nonce: u64,
}
```

## solana specific handling

- **compute budget** - efficient single-instruction operations
- **rent exemption** - automatic via `init` accounts  
- **token account creation** - associated token accounts
- **signer management** - pda signers for program authority

## bounty requirements addressed

✅ solana nft program with cross-chain capabilities  
✅ zetachain gateway integration  
✅ solana-specific challenges (compute, rent, signers)  
✅ evm compatibility via standardized messages  
✅ security best practices (tss/replay protection)  
✅ lock/unlock mechanism preserves nft provenance

program id: `7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU`