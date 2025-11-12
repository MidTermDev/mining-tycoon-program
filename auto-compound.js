/**
 * Auto-Compound Script
 * Automatically compounds hash every 60 seconds
 */

const anchor = require('@coral-xyz/anchor');
const { Connection, PublicKey, Keypair } = require('@solana/web3.js');
const fs = require('fs');
const path = require('path');

const PROGRAM_ID = new PublicKey('t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU');
const RPC_URL = process.env.RPC_URL || 'https://api.mainnet-beta.solana.com';
const COMPOUND_INTERVAL_MS = 60 * 1000; // 60 seconds
const MIN_HASH_TO_COMPOUND = 86400; // Minimum hash needed

async function autoCompound() {
  try {
    // Load keypair
    const keypairPath = process.env.KEYPAIR || path.join(__dirname, '..', 'deploy-keypair.json');
    const keypair = Keypair.fromSecretKey(
      new Uint8Array(JSON.parse(fs.readFileSync(keypairPath, 'utf-8')))
    );

    console.log(`🤖 Auto-Compound Bot Started`);
    console.log(`Wallet: ${keypair.publicKey.toString()}`);
    console.log(`Interval: ${COMPOUND_INTERVAL_MS / 1000} seconds`);
    console.log(`Minimum hash: ${MIN_HASH_TO_COMPOUND.toLocaleString()}`);
    console.log('---\n');

    const connection = new Connection(RPC_URL, 'confirmed');

    // Get PDAs
    const [globalStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('global_state')],
      PROGRAM_ID
    );

    const [userStatePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('user_state'), keypair.publicKey.toBuffer()],
      PROGRAM_ID
    );

    // Immediate first check
    await performCompound(connection, keypair, globalStatePda, userStatePda);

    // Schedule periodic compounds
    setInterval(async () => {
      await performCompound(connection, keypair, globalStatePda, userStatePda);
    }, COMPOUND_INTERVAL_MS);

  } catch (error) {
    console.error('Fatal error:', error);
    process.exit(1);
  }
}

async function performCompound(connection, keypair, globalStatePda, userStatePda) {
  try {
    console.log(`\n[${new Date().toISOString()}] Attempting compound...`);

    // Get discriminator for compound_hash: [2, 19, 201, 206, 143, 188, 100, 120]
    const discriminator = Buffer.from([2, 19, 201, 206, 143, 188, 100, 120]);

    // Create instruction
    const instruction = new anchor.web3.TransactionInstruction({
      keys: [
        { pubkey: globalStatePda, isSigner: false, isWritable: true },
        { pubkey: userStatePda, isSigner: false, isWritable: true },
        { pubkey: keypair.publicKey, isSigner: true, isWritable: true },
      ],
      programId: PROGRAM_ID,
      data: discriminator,
    });

    // Send transaction
    const transaction = new anchor.web3.Transaction().add(instruction);
    const signature = await anchor.web3.sendAndConfirmTransaction(
      connection,
      transaction,
      [keypair],
      { commitment: 'confirmed' }
    );

    console.log(`✅ Compound successful!`);
    console.log(`   Transaction: ${signature}`);

  } catch (error) {
    console.error('Error during compound:', error.message || error);
    // Continue running even if one compound fails
  }
}

// Handle graceful shutdown
process.on('SIGINT', () => {
  console.log('\n⏹️  Shutting down auto-compounder...');
  process.exit(0);
});

process.on('SIGTERM', () => {
  console.log('\n⏹️  Shutting down auto-compounder...');
  process.exit(0);
});

// Start the bot
autoCompound().catch(console.error);
