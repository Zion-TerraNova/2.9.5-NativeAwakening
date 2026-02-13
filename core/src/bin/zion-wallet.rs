use anyhow::{anyhow, Context, Result};
use bip39::{Language, Mnemonic};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "zion-wallet", version, about = "ZION native wallet CLI (skeleton)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate a new ed25519 keypair and write a wallet file.
    Gen {
        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,
    },

    /// Generate a mnemonic-based wallet (seed[:32] -> ed25519), matching mobile/presale.
    GenMnemonic {
        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,

        /// Number of words (12, 15, 18, 21, 24). Default 24.
        #[arg(long, default_value_t = 24)]
        words: u8,

        /// Optional BIP39 passphrase.
        #[arg(long, default_value = "")]
        passphrase: String,
    },

    /// Import a mnemonic and write a wallet file (seed[:32] -> ed25519).
    ImportMnemonic {
        /// Mnemonic words.
        #[arg(long)]
        mnemonic: String,

        /// Optional BIP39 passphrase.
        #[arg(long, default_value = "")]
        passphrase: String,

        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,
    },

    /// Import a raw 32-byte ed25519 secret key (hex) and write a wallet file.
    ImportSecretKey {
        /// Secret key hex (64 hex chars, 32 bytes).
        #[arg(long)]
        secret_key_hex: String,

        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,
    },

    /// Derive address from a public key hex (32 bytes).
    Address {
        /// Public key hex (64 hex chars).
        #[arg(long)]
        public_key_hex: String,
    },

    /// Validate a ZION address format (zion1... Python parity).
    Validate {
        /// Address string.
        #[arg(long)]
        address: String,
    },

    /// Show wallet info from a wallet file.
    Info {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,
    },

    /// Sign a message (hex) with the wallet secret key.
    Sign {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,

        /// Message hex to sign.
        #[arg(long)]
        message_hex: String,
    },

    /// Verify a signature for a message (hex) with a public key.
    Verify {
        /// Public key hex (64 hex chars).
        #[arg(long)]
        public_key_hex: String,

        /// Message hex to verify.
        #[arg(long)]
        message_hex: String,

        /// Signature hex (64 bytes = 128 hex chars).
        #[arg(long)]
        signature_hex: String,
    },

    /// Send ZION to an address (builds, signs, broadcasts transaction).
    Send {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,

        /// Recipient zion1... address.
        #[arg(long)]
        to: String,

        /// Amount to send in ZION (e.g. 100.5).
        #[arg(long)]
        amount: f64,

        /// Optional explicit fee in ZION (default: auto-calculated).
        #[arg(long)]
        fee: Option<f64>,

        /// Node RPC URL to fetch UTXOs and broadcast.
        #[arg(long, default_value = "http://localhost:8545")]
        node: String,

        /// Dry run: build and sign but don't broadcast.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Check balance for an address via node RPC.
    Balance {
        /// Address to check (zion1...).
        #[arg(long)]
        address: String,

        /// Node RPC URL.
        #[arg(long, default_value = "http://localhost:8545")]
        node: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct WalletFile {
    format: String,
    format_version: u32,
    /// WARNING: plaintext secret key (skeleton only).
    secret_key_hex: String,
    public_key_hex: String,
    address: String,
    /// WARNING: plaintext mnemonic (skeleton only).
    #[serde(skip_serializing_if = "Option::is_none")]
    mnemonic: Option<String>,
    created_at_utc: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Gen { out, force, print } => {
            let wallet = generate_wallet_file()?;

            if out.exists() && !force {
                return Err(anyhow!(
                    "Refusing to overwrite existing file: {} (use --force)",
                    out.display()
                ));
            }

            let json = serde_json::to_string_pretty(&wallet)?;
            write_wallet_file(&out, &json)?;

            if print {
                println!("{}", json);
            } else {
                println!("Wrote wallet: {}", out.display());
                println!("Address: {}", wallet.address);
            }

            Ok(())
        }

        Command::GenMnemonic {
            out,
            force,
            print,
            words,
            passphrase,
        } => {
            let wallet = generate_mnemonic_wallet_file(words, &passphrase)?;

            if out.exists() && !force {
                return Err(anyhow!(
                    "Refusing to overwrite existing file: {} (use --force)",
                    out.display()
                ));
            }

            let json = serde_json::to_string_pretty(&wallet)?;
            write_wallet_file(&out, &json)?;

            if print {
                println!("{}", json);
            } else {
                println!("Wrote wallet: {}", out.display());
                println!("Address: {}", wallet.address);
            }

            Ok(())
        }

        Command::ImportMnemonic {
            mnemonic,
            passphrase,
            out,
            force,
            print,
        } => {
            let wallet = import_mnemonic_wallet_file(&mnemonic, &passphrase)?;

            if out.exists() && !force {
                return Err(anyhow!(
                    "Refusing to overwrite existing file: {} (use --force)",
                    out.display()
                ));
            }

            let json = serde_json::to_string_pretty(&wallet)?;
            write_wallet_file(&out, &json)?;

            if print {
                println!("{}", json);
            } else {
                println!("Wrote wallet: {}", out.display());
                println!("Address: {}", wallet.address);
            }

            Ok(())
        }

        Command::ImportSecretKey {
            secret_key_hex,
            out,
            force,
            print,
        } => {
            let wallet = import_secret_key_wallet_file(&secret_key_hex)?;

            if out.exists() && !force {
                return Err(anyhow!(
                    "Refusing to overwrite existing file: {} (use --force)",
                    out.display()
                ));
            }

            let json = serde_json::to_string_pretty(&wallet)?;
            write_wallet_file(&out, &json)?;

            if print {
                println!("{}", json);
            } else {
                println!("Wrote wallet: {}", out.display());
                println!("Address: {}", wallet.address);
            }

            Ok(())
        }

        Command::Address { public_key_hex } => {
            let address = zion_core::crypto::keys::address_from_public_key_hex(&public_key_hex);
            println!("{}", address);
            Ok(())
        }

        Command::Validate { address } => {
            let ok = zion_core::crypto::keys::is_valid_zion1_address(address.trim());
            if ok {
                println!("OK");
                Ok(())
            } else {
                Err(anyhow!("Invalid address format (expected zion1 + 39 [a-z0-9] chars)"))
            }
        }

        Command::Info { wallet } => {
            let w = read_wallet_file(&wallet)?;
            println!("Wallet: {}", wallet.display());
            println!("Public key: {}", w.public_key_hex);
            println!("Address: {}", w.address);
            println!("Mnemonic: {}", if w.mnemonic.is_some() { "yes" } else { "no" });
            println!("Created: {}", w.created_at_utc);
            Ok(())
        }

        Command::Sign { wallet, message_hex } => {
            let w = read_wallet_file(&wallet)?;
            let msg = hex_to_bytes(&message_hex).context("message_hex must be valid hex")?;
            let sk_bytes = hex_to_32(&w.secret_key_hex).context("secret_key_hex must be 32-byte hex")?;

            let signing_key = SigningKey::from_bytes(&sk_bytes);
            let sig: Signature = signing_key.sign(&msg);
            println!("{}", bytes_to_hex(sig.to_bytes().as_slice()));
            Ok(())
        }

        Command::Verify {
            public_key_hex,
            message_hex,
            signature_hex,
        } => {
            let msg = hex_to_bytes(&message_hex).context("message_hex must be valid hex")?;
            let pk_bytes = hex_to_32(&public_key_hex).context("public_key_hex must be 32-byte hex")?;
            let sig_bytes = hex_to_64(&signature_hex).context("signature_hex must be 64-byte hex")?;

            let verifying_key = VerifyingKey::from_bytes(&pk_bytes)?;
            let signature = Signature::from_bytes(&sig_bytes);

            verifying_key
                .verify_strict(&msg, &signature)
                .map_err(|e| anyhow!("Signature invalid: {e}"))?;

            println!("OK");
            Ok(())
        }

        Command::Send {
            wallet,
            to,
            amount,
            fee,
            node,
            dry_run,
        } => {
            use zion_core::wallet;

            let w = read_wallet_file(&wallet)?;
            let sk_bytes = hex_to_32(&w.secret_key_hex)
                .ok_or_else(|| anyhow!("Invalid secret key in wallet file"))?;

            // Convert ZION amounts to atomic units (1 ZION = 1,000,000 atomic)
            let amount_atomic = (amount * 1_000_000.0) as u64;
            let fee_atomic = fee.map(|f| (f * 1_000_000.0) as u64);

            if amount_atomic == 0 {
                return Err(anyhow!("Amount must be > 0"));
            }

            // Fetch UTXOs from node
            println!("Fetching UTXOs from {}...", node);
            let utxos_url = format!("{}/api/address/{}/utxos?limit=500", node, w.address);
            let client = reqwest::blocking::Client::new();
            let resp: serde_json::Value = client
                .get(&utxos_url)
                .send()
                .context("Failed to connect to node")?
                .json()
                .context("Failed to parse UTXO response")?;

            let utxo_list = resp["utxos"]
                .as_array()
                .ok_or_else(|| anyhow!("No 'utxos' array in response"))?;

            let available: Vec<wallet::SpendableUtxo> = utxo_list
                .iter()
                .filter_map(|u| {
                    let key = u["key"].as_str()?;
                    let (tx_hash, output_index) = wallet::parse_utxo_key(key)?;
                    Some(wallet::SpendableUtxo {
                        key: key.to_string(),
                        tx_hash,
                        output_index,
                        amount: u["amount"].as_u64()?,
                        address: u["address"].as_str()?.to_string(),
                    })
                })
                .collect();

            println!("Found {} UTXOs for {}", available.len(), w.address);

            if available.is_empty() {
                return Err(anyhow!("No spendable UTXOs found for {}", w.address));
            }

            let params = wallet::SendParams {
                to_address: to.clone(),
                amount: amount_atomic,
                fee: fee_atomic,
                change_address: w.address.clone(),
            };

            let result = wallet::build_and_sign(&params, &available, &sk_bytes)
                .map_err(|e| anyhow!("Build failed: {}", e))?;

            println!("\n--- Transaction Built ---");
            println!("TX ID:      {}", result.transaction.id);
            println!("To:         {}", to);
            println!("Amount:     {:.6} ZION", result.amount_sent as f64 / 1_000_000.0);
            println!("Fee:        {:.6} ZION (burned)", result.fee as f64 / 1_000_000.0);
            println!("Change:     {:.6} ZION", result.change as f64 / 1_000_000.0);
            println!("Inputs:     {}", result.inputs_used);
            println!("Outputs:    {}", result.transaction.outputs.len());

            if dry_run {
                println!("\n[DRY RUN] Transaction NOT broadcast.");
                let tx_json = serde_json::to_string_pretty(&result.transaction)?;
                println!("{}", tx_json);
            } else {
                println!("\nBroadcasting to {}...", node);
                let submit_url = format!("{}/rpc/submit_tx", node);
                let resp: serde_json::Value = client
                    .post(&submit_url)
                    .json(&result.transaction)
                    .send()
                    .context("Failed to broadcast transaction")?
                    .json()
                    .context("Failed to parse broadcast response")?;

                if resp["status"].as_str() == Some("ok") {
                    println!("✅ Transaction broadcast successfully!");
                    println!("TX ID: {}", result.transaction.id);
                } else {
                    let msg = resp["message"].as_str().unwrap_or("Unknown error");
                    return Err(anyhow!("Broadcast failed: {}", msg));
                }
            }

            Ok(())
        }

        Command::Balance { address, node } => {
            let url = format!("{}/api/address/{}/balance", node, address);
            let client = reqwest::blocking::Client::new();
            let resp: serde_json::Value = client
                .get(&url)
                .send()
                .context("Failed to connect to node")?
                .json()
                .context("Failed to parse balance response")?;

            if resp["status"].as_str() == Some("ok") {
                let atomic = resp["balance_atomic"].as_u64().unwrap_or(0);
                let zion = atomic as f64 / 1_000_000.0;
                let utxo_count = resp["utxo_count"].as_u64().unwrap_or(0);
                println!("Address: {}", address);
                println!("Balance: {:.6} ZION ({} atomic)", zion, atomic);
                println!("UTXOs:   {}", utxo_count);
            } else {
                let msg = resp["message"].as_str().unwrap_or("Unknown error");
                return Err(anyhow!("Balance check failed: {}", msg));
            }

            Ok(())
        }
    }
}

fn generate_wallet_file() -> Result<WalletFile> {
    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);

    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();

    let secret_key_hex = bytes_to_hex(&secret);
    let public_key_hex = bytes_to_hex(verifying_key.as_bytes());
    let address = zion_core::crypto::keys::address_from_public_key_hex(&public_key_hex);

    Ok(WalletFile {
        format: "zion-wallet".to_string(),
        format_version: 1,
        secret_key_hex,
        public_key_hex,
        address,
        mnemonic: None,
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn generate_mnemonic_wallet_file(words: u8, passphrase: &str) -> Result<WalletFile> {
    let word_count = match words {
        12 | 15 | 18 | 21 | 24 => words as usize,
        _ => return Err(anyhow!("Unsupported word count: {} (use 12/15/18/21/24)", words)),
    };

    let mnemonic = Mnemonic::generate_in_with(&mut OsRng, Language::English, word_count)
        .map_err(|e| anyhow!("Failed to generate mnemonic: {e}"))?;
    let phrase = mnemonic.to_string();
    import_mnemonic_wallet_file(&phrase, passphrase)
}

fn import_mnemonic_wallet_file(mnemonic: &str, passphrase: &str) -> Result<WalletFile> {
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)
        .map_err(|e| anyhow!("Invalid mnemonic: {e}"))?;

    let seed = mnemonic.to_seed(passphrase);
    let secret: [u8; 32] = seed[0..32]
        .try_into()
        .map_err(|_| anyhow!("Seed slice conversion failed"))?;

    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();

    let secret_key_hex = bytes_to_hex(&secret);
    let public_key_hex = bytes_to_hex(verifying_key.as_bytes());
    let address = zion_core::crypto::keys::address_from_public_key_hex(&public_key_hex);

    Ok(WalletFile {
        format: "zion-wallet".to_string(),
        format_version: 1,
        secret_key_hex,
        public_key_hex,
        address,
        mnemonic: Some(mnemonic.to_string()),
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn import_secret_key_wallet_file(secret_key_hex: &str) -> Result<WalletFile> {
    let secret = hex_to_32(secret_key_hex).context("secret_key_hex must be 32-byte hex")?;

    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();

    let secret_key_hex = bytes_to_hex(&secret);
    let public_key_hex = bytes_to_hex(verifying_key.as_bytes());
    let address = zion_core::crypto::keys::address_from_public_key_hex(&public_key_hex);

    Ok(WalletFile {
        format: "zion-wallet".to_string(),
        format_version: 1,
        secret_key_hex,
        public_key_hex,
        address,
        mnemonic: None,
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn write_wallet_file(path: &Path, json: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
        }
    }

    fs::write(path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn read_wallet_file(path: &Path) -> Result<WalletFile> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed: WalletFile = serde_json::from_str(&raw).context("parse wallet JSON")?;
    Ok(parsed)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte_str = &s[i..i + 2];
        let b = u8::from_str_radix(byte_str, 16).ok()?;
        bytes.push(b);
    }
    Some(bytes)
}

fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    let v = hex_to_bytes(s)?;
    if v.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Some(out)
}

fn hex_to_64(s: &str) -> Option<[u8; 64]> {
    let v = hex_to_bytes(s)?;
    if v.len() != 64 {
        return None;
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&v);
    Some(out)
}
