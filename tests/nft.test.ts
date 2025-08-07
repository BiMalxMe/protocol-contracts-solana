import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { UniversalNft } from "../target/types/universal_nft";
import { 
  PublicKey, 
  Keypair, 
  SystemProgram,
  SYSVAR_RENT_PUBKEY 
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddress,
  createAssociatedTokenAccount,
  getAccount
} from "@solana/spl-token";
import { expect } from "chai";
import { BN } from "bn.js";

// metaplex metadata program id
const METADATA_PROGRAM_ID = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

describe("universal nft", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.UniversalNft as Program<UniversalNft>;
  
  // test keypairs
  const authority = Keypair.generate();
  const user = Keypair.generate();
  const recipient = Keypair.generate();
  
  // program pdas
  let nftProgramPda: PublicKey;
  let nftProgramBump: number;
  let gatewayPda: PublicKey;
  
  // nft data
  const nftName = "test nft";
  const nftSymbol = "TEST";
  const nftUri = "https://test.com/metadata.json";
  
  // test mint keypair
  const mint = Keypair.generate();
  let nftInfoPda: PublicKey;
  let nftInfoBump: number;
  let metadataPda: PublicKey;
  let tokenAccount: PublicKey;
  let programTokenAccount: PublicKey;

  before(async () => {
    // airdrop sol to test accounts
    await provider.connection.requestAirdrop(authority.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(recipient.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    
    // wait for airdrops
    await new Promise(resolve => setTimeout(resolve, 1000));
    
    // find pdas
    [nftProgramPda, nftProgramBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("nft-program")],
      program.programId
    );
    
    [nftInfoPda, nftInfoBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("nft-info"), mint.publicKey.toBuffer()],
      program.programId
    );
    
    [metadataPda] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("metadata"),
        METADATA_PROGRAM_ID.toBuffer(),
        mint.publicKey.toBuffer(),
      ],
      METADATA_PROGRAM_ID
    );
    
    // mock gateway pda
    gatewayPda = Keypair.generate().publicKey;
    
    // token accounts
    tokenAccount = await getAssociatedTokenAddress(mint.publicKey, recipient.publicKey);
    programTokenAccount = await getAssociatedTokenAddress(mint.publicKey, nftProgramPda, true);
  });

  describe("initialization", () => {
    it("initializes the universal nft program", async () => {
      const tx = await program.methods
        .initialize(gatewayPda)
        .accounts({
          nftProgram: nftProgramPda,
          authority: authority.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .signers([authority])
        .rpc();

      console.log("initialize tx:", tx);

      // verify program state
      const programState = await program.account.nftProgramState.fetch(nftProgramPda);
      expect(programState.authority.toString()).to.equal(authority.publicKey.toString());
      expect(programState.gateway.toString()).to.equal(gatewayPda.toString());
      expect(programState.totalSupply.toString()).to.equal("0");
      expect(programState.nonce.toString()).to.equal("0");
    });
  });

  describe("nft minting", () => {
    it("mints a new nft with metadata", async () => {
      const tx = await program.methods
        .mintNft(nftName, nftSymbol, nftUri, recipient.publicKey)
        .accounts({
          nftProgram: nftProgramPda,
          mint: mint.publicKey,
          tokenAccount: tokenAccount,
          nftInfo: nftInfoPda,
          metadata: metadataPda,
          payer: authority.publicKey,
          rent: SYSVAR_RENT_PUBKEY,
          systemProgram: SystemProgram.programId,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          tokenMetadataProgram: METADATA_PROGRAM_ID,
        })
        .signers([authority, mint])
        .rpc();

      console.log("mint nft tx:", tx);

      // verify nft was minted
      const tokenAccountInfo = await getAccount(provider.connection, tokenAccount);
      expect(tokenAccountInfo.amount.toString()).to.equal("1");
      expect(tokenAccountInfo.mint.toString()).to.equal(mint.publicKey.toString());

      // verify nft info
      const nftInfo = await program.account.nftInfo.fetch(nftInfoPda);
      expect(nftInfo.mint.toString()).to.equal(mint.publicKey.toString());
      expect(nftInfo.owner.toString()).to.equal(recipient.publicKey.toString());
      expect(nftInfo.name).to.equal(nftName);
      expect(nftInfo.symbol).to.equal(nftSymbol);
      expect(nftInfo.metadataUri).to.equal(nftUri);
      expect(nftInfo.isLocked).to.be.false;

      // verify program state updated
      const programState = await program.account.nftProgramState.fetch(nftProgramPda);
      expect(programState.totalSupply.toString()).to.equal("1");
    });
  });

  describe("cross-chain transfer", () => {
    const destinationChainId = new BN(7001); // zetachain testnet
    const evmRecipient = Array.from(Buffer.alloc(32, 1)); // mock evm address
    const nonce = new BN(Date.now());

    it("initiates cross-chain transfer to zetachain", async () => {
      // create program token account first
      await createAssociatedTokenAccount(
        provider.connection,
        authority,
        mint.publicKey,
        nftProgramPda,
        {},
        TOKEN_PROGRAM_ID,
        ASSOCIATED_TOKEN_PROGRAM_ID
      );

      const tx = await program.methods
        .transferToZetachain(destinationChainId, evmRecipient, nonce)
        .accounts({
          nftProgram: nftProgramPda,
          nftInfo: nftInfoPda,
          owner: recipient.publicKey,
          ownerTokenAccount: tokenAccount,
          programTokenAccount: programTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
        })
        .signers([recipient])
        .rpc();

      console.log("transfer to zetachain tx:", tx);

      // verify nft was locked (transferred to program)
      const ownerTokenAccountInfo = await getAccount(provider.connection, tokenAccount);
      expect(ownerTokenAccountInfo.amount.toString()).to.equal("0");

      const programTokenAccountInfo = await getAccount(provider.connection, programTokenAccount);
      expect(programTokenAccountInfo.amount.toString()).to.equal("1");

      // verify nft info updated
      const nftInfo = await program.account.nftInfo.fetch(nftInfoPda);
      expect(nftInfo.isLocked).to.be.true;
      expect(Array.from(nftInfo.crossChainRecipient)).to.deep.equal(evmRecipient);

      // verify program nonce updated
      const programState = await program.account.nftProgramState.fetch(nftProgramPda);
      expect(programState.nonce.toString()).to.equal(nonce.toString());
    });

    it("handles cross-chain call from zetachain", async () => {
      const sender = Array.from(Buffer.alloc(32, 2));
      const sourceChainId = new BN(7001);
      const newNonce = nonce.add(new BN(1));
      
      // mock cross-chain message
      const crossChainMessage = {
        messageType: { transfer: {} },
        mint: mint.publicKey,
        recipient: evmRecipient,
        metadataUri: nftUri,
        name: nftName,
        symbol: nftSymbol,
        nonce: newNonce,
      };
      
      const messageBuffer = Buffer.from(JSON.stringify(crossChainMessage));

      const tx = await program.methods
        .handleCrossChainCall(sender, sourceChainId, Array.from(messageBuffer), newNonce)
        .accounts({
          nftProgram: nftProgramPda,
        })
        .signers([authority])
        .rpc();

      console.log("handle cross-chain call tx:", tx);

      // verify nonce updated
      const programState = await program.account.nftProgramState.fetch(nftProgramPda);
      expect(programState.nonce.toString()).to.equal(newNonce.toString());
    });

    it("unlocks nft after cross-chain return", async () => {
      const unlockNonce = new BN(Date.now() + 1000);

      const tx = await program.methods
        .unlockNft(unlockNonce)
        .accounts({
          nftProgram: nftProgramPda,
          nftInfo: nftInfoPda,
          owner: recipient.publicKey,
          ownerTokenAccount: tokenAccount,
          programTokenAccount: programTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([recipient])
        .rpc();

      console.log("unlock nft tx:", tx);

      // verify nft returned to owner
      const ownerTokenAccountInfo = await getAccount(provider.connection, tokenAccount);
      expect(ownerTokenAccountInfo.amount.toString()).to.equal("1");

      const programTokenAccountInfo = await getAccount(provider.connection, programTokenAccount);
      expect(programTokenAccountInfo.amount.toString()).to.equal("0");

      // verify nft info updated
      const nftInfo = await program.account.nftInfo.fetch(nftInfoPda);
      expect(nftInfo.isLocked).to.be.false;

      // verify program nonce updated
      const programState = await program.account.nftProgramState.fetch(nftProgramPda);
      expect(programState.nonce.toString()).to.equal(unlockNonce.toString());
    });
  });

  describe("security tests", () => {
    it("prevents unauthorized transfers", async () => {
      const unauthorizedUser = Keypair.generate();
      await provider.connection.requestAirdrop(unauthorizedUser.publicKey, anchor.web3.LAMPORTS_PER_SOL);
      await new Promise(resolve => setTimeout(resolve, 500));

      try {
        await program.methods
          .transferToZetachain(new BN(7001), Array.from(Buffer.alloc(32, 1)), new BN(Date.now()))
          .accounts({
            nftProgram: nftProgramPda,
            nftInfo: nftInfoPda,
            owner: unauthorizedUser.publicKey,
            ownerTokenAccount: tokenAccount,
            programTokenAccount: programTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .signers([unauthorizedUser])
          .rpc();
        
        expect.fail("should have failed with unauthorized error");
      } catch (error) {
        expect(error.message).to.include("Unauthorized");
      }
    });

    it("prevents replay attacks with old nonce", async () => {
      const oldNonce = new BN(1); // old nonce

      try {
        await program.methods
          .handleCrossChainCall(
            Array.from(Buffer.alloc(32, 2)),
            new BN(7001),
            Array.from(Buffer.from("test message")),
            oldNonce
          )
          .accounts({
            nftProgram: nftProgramPda,
          })
          .signers([authority])
          .rpc();
        
        expect.fail("should have failed with invalid nonce error");
      } catch (error) {
        expect(error.message).to.include("InvalidNonce");
      }
    });

    it("prevents unlocking non-locked nfts", async () => {
      try {
        await program.methods
          .unlockNft(new BN(Date.now()))
          .accounts({
            nftProgram: nftProgramPda,
            nftInfo: nftInfoPda,
            owner: recipient.publicKey,
            ownerTokenAccount: tokenAccount,
            programTokenAccount: programTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .signers([recipient])
          .rpc();
        
        expect.fail("should have failed with token not locked error");
      } catch (error) {
        expect(error.message).to.include("TokenNotLocked");
      }
    });
  });

  describe("edge cases", () => {
    it("handles invalid metadata inputs", async () => {
      const longName = "a".repeat(50); // exceeds max length
      const newMint = Keypair.generate();

      try {
        await program.methods
          .mintNft(longName, nftSymbol, nftUri, recipient.publicKey)
          .accounts({
            nftProgram: nftProgramPda,
            mint: newMint.publicKey,
            tokenAccount: await getAssociatedTokenAddress(newMint.publicKey, recipient.publicKey),
            nftInfo: PublicKey.findProgramAddressSync(
              [Buffer.from("nft-info"), newMint.publicKey.toBuffer()],
              program.programId
            )[0],
            metadata: PublicKey.findProgramAddressSync(
              [
                Buffer.from("metadata"),
                METADATA_PROGRAM_ID.toBuffer(),
                newMint.publicKey.toBuffer(),
              ],
              METADATA_PROGRAM_ID
            )[0],
            payer: authority.publicKey,
            rent: SYSVAR_RENT_PUBKEY,
            systemProgram: SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            tokenMetadataProgram: METADATA_PROGRAM_ID,
          })
          .signers([authority, newMint])
          .rpc();
        
        expect.fail("should have failed with invalid metadata error");
      } catch (error) {
        expect(error.message).to.include("InvalidMetadata");
      }
    });

    it("validates cross-chain message parsing", async () => {
      const invalidMessage = Array.from(Buffer.from("invalid json"));
      const newNonce = new BN(Date.now() + 2000);

      try {
        await program.methods
          .handleCrossChainCall(
            Array.from(Buffer.alloc(32, 2)),
            new BN(7001),
            invalidMessage,
            newNonce
          )
          .accounts({
            nftProgram: nftProgramPda,
          })
          .signers([authority])
          .rpc();
        
        expect.fail("should have failed with invalid message error");
      } catch (error) {
        expect(error.message).to.include("InvalidMessage");
      }
    });
  });
});