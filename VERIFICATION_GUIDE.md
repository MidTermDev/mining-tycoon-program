# Program Verification Guide

## Current Status
✅ Verifiable build completed successfully  
✅ Source code synced to mining-tycoon-opensource repository  
⏳ Ready to push and verify

## Program Details
- **Program ID**: `t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU`
- **Program Name**: `bakedbeans_solana`
- **Solana Version**: v2.3.0

## Changes Made
1. Added `anchor-spl = "0.31.1"` dependency to `programs/bakedbeans_solana/Cargo.toml`
2. Copied the deployed program source code from `bakedbeans_solana` directory
3. Generated `Cargo.lock` file for reproducible builds

## Next Steps

### 1. Review Changes
```bash
cd mining-tycoon-opensource
git diff programs/bakedbeans_solana/Cargo.toml
```

### 2. Commit and Push
```bash
git add .
git commit -m "Update source code for verified builds - add anchor-spl dependency and Cargo.lock"
git push origin main
```

### 3. Verify the Program
Once pushed, run the verification command (replace `YOUR_GITHUB_USERNAME` with your actual username):

```bash
solana-verify verify-from-repo \
  --program-id t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU \
  https://github.com/YOUR_GITHUB_USERNAME/mining-tycoon-opensource \
  --library-name bakedbeans_solana \
  --mount-path .
```

### Alternative: Build and Verify Locally
If you want to verify without pushing first:

```bash
cd mining-tycoon-opensource
solana-verify build --library-name bakedbeans_solana

# Then get the executable hash
solana-verify get-executable-hash target/deploy/bakedbeans_solana.so

# Compare with on-chain hash
solana-verify get-program-hash t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU
```

## Expected Output
If verification succeeds, you'll see:
```
Executable hash matches on-chain program data ✓
Program t6YG88Q2wCsimhQ5gqSeRC8Wm5qVksw62urHAezPGPU verified successfully!
```

## Troubleshooting

### If hashes don't match:
1. Ensure you're using the exact same Rust toolchain version (check with `rustc --version`)
2. Verify Solana CLI version matches: `solana --version` (should be v2.3.0 or compatible)
3. Make sure all dependencies in Cargo.toml are exactly as they were during deployment

### If build fails:
1. Run `cargo clean` and try again
2. Ensure all dependencies are available: `cargo fetch`
3. Check that anchor-cli version matches: `anchor --version`

## Additional Resources
- [Solana Verified Builds Documentation](https://solana.com/docs/programs/verified-builds)
- [Solana Verify CLI GitHub](https://github.com/Ellipsis-Labs/solana-verifiable-build)

## Notes
- The verification process uses Docker to ensure reproducible builds
- The built binary must match byte-for-byte with the on-chain program
- This process allows users to verify that the on-chain program matches the published source code
