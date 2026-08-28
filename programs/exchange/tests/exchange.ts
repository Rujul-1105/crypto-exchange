import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import { assert } from "chai";
import { Exchange } from "../target/types/exchange";

const WSOL_MINT_PUBKEY = new PublicKey("So11111111111111111111111111111111111111112");

describe("exchange", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Exchange as Program<Exchange>;
  const admin = (provider.wallet as anchor.Wallet).payer;
  const userA = Keypair.generate();
  const userB = Keypair.generate();

  let usdcMint: PublicKey;
  let configPda: PublicKey;
  let vaultAuthority: PublicKey;
  let userABalance: PublicKey;
  let userBBalance: PublicKey;

  before(async () => {
    // Airdrop SOL to test users.
    const sigA = await provider.connection.requestAirdrop(userA.publicKey, 10e9);
    const sigB = await provider.connection.requestAirdrop(userB.publicKey, 10e9);
    await Promise.all([
      provider.connection.confirmTransaction(sigA),
      provider.connection.confirmTransaction(sigB),
    ]);

    // Create a USDC mint for the test (wSOL uses the real mainnet wSOL mint).
    usdcMint = await createMint(provider.connection, admin, admin.publicKey, null, 6);

    [configPda] = PublicKey.findProgramAddressSync([Buffer.from("config")], program.programId);
    [vaultAuthority] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_authority")],
      program.programId,
    );
    [userABalance] = PublicKey.findProgramAddressSync(
      [Buffer.from("user_balance"), userA.publicKey.toBuffer()],
      program.programId,
    );
    [userBBalance] = PublicKey.findProgramAddressSync(
      [Buffer.from("user_balance"), userB.publicKey.toBuffer()],
      program.programId,
    );
  });

  it("initializes the exchange config", async () => {
    await program.methods
      .initialize(10) // 10 bps fee
      .accounts({
        config: configPda,
        vaultAuthority,
        admin: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const config = await program.account.config.fetch(configPda);
    assert.equal(config.admin.toBase58(), admin.publicKey.toBase58());
    assert.equal(config.vaultAuthority.toBase58(), vaultAuthority.toBase58());
    assert.equal(config.feeBps, 10);
    assert.equal(config.paused, false);
  });

  it("deposits USDC for user A and user B", async () => {
    const sharedUsdcVault = getAssociatedTokenAddressSync(usdcMint, vaultAuthority, true);

    const ataA = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      admin,
      usdcMint,
      userA.publicKey,
    );
    const ataB = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      admin,
      usdcMint,
      userB.publicKey,
    );

    // Mint 1000 USDC to each user.
    await mintTo(provider.connection, admin, usdcMint, ataA.address, admin, 1_000_000_000); // 1000 USDC
    await mintTo(provider.connection, admin, usdcMint, ataB.address, admin, 1_000_000_000);

    // Now call deposit_usdc.
    const depositAmount = 500_000_000; // 500 USDC
    await program.methods
      .depositUsdc(new anchor.BN(depositAmount))
      .accounts({
        user: userA.publicKey,
        config: configPda,
        vaultAuthority,
        sharedUsdcVault,
        userUsdcAta: ataA.address,
        userBalance: userABalance,
        usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([userA])
      .rpc();

    await program.methods
      .depositUsdc(new anchor.BN(depositAmount))
      .accounts({
        user: userB.publicKey,
        config: configPda,
        vaultAuthority,
        sharedUsdcVault,
        userUsdcAta: ataB.address,
        userBalance: userBBalance,
        usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([userB])
      .rpc();

    const balanceA = await program.account.userBalance.fetch(userABalance);
    const balanceB = await program.account.userBalance.fetch(userBBalance);
    assert.equal(balanceA.usdc.toNumber(), depositAmount);
    assert.equal(balanceB.usdc.toNumber(), depositAmount);
    assert.equal(balanceA.sol.toNumber(), 0);
    assert.equal(balanceB.sol.toNumber(), 0);
  });

  it("settles a fill: buyer A gets SOL, seller B gets USDC", async () => {
    // Setup: also need both users to have wSOL to trade. For this test, we
    // mint wSOL via the actual wSOL mint.
    const sharedWsolVault = getAssociatedTokenAddressSync(WSOL_MINT_PUBKEY, vaultAuthority, true);
    const userAWsolAta = getAssociatedTokenAddressSync(WSOL_MINT_PUBKEY, userA.publicKey);
    const userBWsolAta = getAssociatedTokenAddressSync(WSOL_MINT_PUBKEY, userB.publicKey);

    // Fund both users' wSOL ATAs by sending native SOL (test validator mints wSOL by default
    // when SOL is sent to the wSOL mint — but for simplicity we'll mint to user B's wSOL ATA
    // via spl-token after wrapping). For this test, we use spl-token to mint directly to user B
    // since we own the validator.
    await mintTo(provider.connection, admin, WSOL_MINT_PUBKEY, userBWsolAta, admin, 1e9);

    // user B deposits wSOL so they have SOL balance.
    await program.methods
      .depositSol(new anchor.BN(1e9))
      .accounts({
        user: userB.publicKey,
        config: configPda,
        vaultAuthority,
        sharedSolVault: sharedWsolVault,
        userWsolAta: userBWsolAta,
        userBalance: userBBalance,
        wsolMint: WSOL_MINT_PUBKEY,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .signers([userB])
      .rpc();

    const buyOrderId = new anchor.BN(1);
    const sellOrderId = new anchor.BN(2);
    const price = new anchor.BN(100_000_000); // 100 USDC per SOL (6 decimals)
    const quantity = new anchor.BN(500_000_000); // 0.5 SOL (9 decimals)
    const notional = price.mul(quantity);

    await program.methods
      .settleFill(buyOrderId, sellOrderId, price, quantity)
      .accounts({
        settler: admin.publicKey,
        config: configPda,
        settlementRecord: PublicKey.findProgramAddressSync(
          [
            Buffer.from("settlement"),
            buyOrderId.toArrayLike(Buffer, "le", 8),
            sellOrderId.toArrayLike(Buffer, "le", 8),
          ],
          program.programId,
        )[0],
        buyerBalance: userABalance,
        sellerBalance: userBBalance,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const a = await program.account.userBalance.fetch(userABalance);
    const b = await program.account.userBalance.fetch(userBBalance);

    // Buyer A: started with 500 USDC, spent 100 * 0.5 = 50 USDC; gained 0.5 SOL.
    assert.equal(a.usdc.toNumber(), 500_000_000 - notional.toNumber());
    assert.equal(a.sol.toNumber(), quantity.toNumber());
    // Seller B: started with 500 USDC + 1 SOL, spent 0.5 SOL; gained 50 USDC.
    assert.equal(b.usdc.toNumber(), 500_000_000 + notional.toNumber());
    assert.equal(b.sol.toNumber(), 1e9 - quantity.toNumber());
  });

  it("rejects duplicate settle_fill (idempotency)", async () => {
    const buyOrderId = new anchor.BN(1);
    const sellOrderId = new anchor.BN(2);
    const price = new anchor.BN(100_000_000);
    const quantity = new anchor.BN(500_000_000);

    let threw = false;
    try {
      await program.methods
        .settleFill(buyOrderId, sellOrderId, price, quantity)
        .accounts({
          settler: admin.publicKey,
          config: configPda,
          settlementRecord: PublicKey.findProgramAddressSync(
            [
              Buffer.from("settlement"),
              buyOrderId.toArrayLike(Buffer, "le", 8),
              sellOrderId.toArrayLike(Buffer, "le", 8),
            ],
            program.programId,
          )[0],
          buyerBalance: userABalance,
          sellerBalance: userBBalance,
          systemProgram: SystemProgram.programId,
        })
        .rpc();
    } catch (e) {
      threw = true;
    }
    assert.isTrue(threw, "second settle_fill with same IDs must fail");
  });

  it("rejects self-trade (buyer == seller)", async () => {
    const buyOrderId = new anchor.BN(10);
    const sellOrderId = new anchor.BN(11);
    const price = new anchor.BN(100_000_000);
    const quantity = new anchor.BN(1_000_000);

    let threw = false;
    try {
      await program.methods
        .settleFill(buyOrderId, sellOrderId, price, quantity)
        .accounts({
          settler: admin.publicKey,
          config: configPda,
          settlementRecord: PublicKey.findProgramAddressSync(
            [
              Buffer.from("settlement"),
              buyOrderId.toArrayLike(Buffer, "le", 8),
              sellOrderId.toArrayLike(Buffer, "le", 8),
            ],
            program.programId,
          )[0],
          buyerBalance: userABalance,
          sellerBalance: userABalance, // self
          systemProgram: SystemProgram.programId,
        })
        .rpc();
    } catch (e) {
      threw = true;
    }
    assert.isTrue(threw, "self-trade must fail");
  });
});
