// call `initialize` on the devnet-deployed program.

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { Exchange } from "../target/types/exchange";

const PROGRAM_ID = new PublicKey("DNhvifJ6mcgVRRA4xaNKH82tjoHseN3GQKiLi8R7i6HY");
const FEE_BPS = 30; // 0.30%

async function main() {
    // Connect to devnet via the configured Solana wallet.
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.Exchange as Program<Exchange>;
    const admin = (provider.wallet as anchor.Wallet).payer;

    // Derive the two PDAs client-side. Anchor will validate these against
    // the supplied accounts at instruction entry, but we compute them here
    // so the .accounts({...}) call has the right pubkeys to pass in.
    const [configPda] = PublicKey.findProgramAddressSync([Buffer.from("config")], PROGRAM_ID);
    const [vaultAuthority] = PublicKey.findProgramAddressSync(
        [Buffer.from("vault_authority")],
        PROGRAM_ID
    );

    console.log("Program:    ", PROGRAM_ID.toBase58());
    console.log("Admin:      ", admin.publicKey.toBase58());
    console.log("Config PDA: ", configPda.toBase58());
    console.log("VaultAuth:  ", vaultAuthority.toBase58());
    console.log("Fee bps:    ", FEE_BPS);
    console.log("---");

    // Call initialize(fee_bps = 30). The accounts context for `Initialize`
    // (lib.rs / instructions.rs) is: config (init), vault_authority (PDA
    // CHECK), admin (Signer, mut, payer), system_program. Anchor derives
    // `config`, `vaultAuthority` (PDAs) and `systemProgram` (fixed address)
    // from the IDL automatically, so we only need to pass the signer.
    const sig = await program.methods
        .initialize(FEE_BPS)
        .accounts({
            admin: admin.publicKey,
        })
        .rpc();

    console.log("initialize signature:", sig);
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
