use std::{fmt, ops::Deref, str::FromStr};

use anyhow::{Context, Result};
use integration_test_tools::IntegrationTestContext;
use schedulecommit_program::api::{
    delegate_account_cpi_instruction, init_account_instruction, pda_and_bump,
};
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
#[allow(unused_imports)]
use solana_sdk::signer::SeedDerivable;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::Hash,
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

pub struct ScheduleCommitTestContext {
    // The first payer from the committees array which is used to fund transactions
    pub payer: Keypair,
    // The Payer keypairs along with its PDA pubkey which we'll commit
    pub committees: Vec<(Keypair, Pubkey)>,

    common_ctx: IntegrationTestContext,
}

impl Default for ScheduleCommitTestContext {
    fn default() -> Self {
        Self::new(1)
    }
}

impl fmt::Display for ScheduleCommitTestContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ScheduleCommitTestContext {{ committees: [")?;
        for (payer, pda) in &self.committees {
            writeln!(f, "Payer: {} PDA: {}, ", payer.pubkey(), pda)?;
        }
        writeln!(f, "] }}")
    }
}

pub struct ScheduleCommitTestContextFields<'a> {
    pub payer: &'a Keypair,
    pub committees: &'a Vec<(Keypair, Pubkey)>,
    pub commitment: &'a CommitmentConfig,
    pub chain_client: &'a RpcClient,
    pub ephem_client: &'a RpcClient,
    pub validator_identity: &'a Pubkey,
    pub chain_blockhash: &'a Hash,
    pub ephem_blockhash: &'a Hash,
}

impl ScheduleCommitTestContext {
    // -----------------
    // Init
    // -----------------
    pub fn new_random_keys(ncommittees: usize) -> Self {
        Self::new_internal(ncommittees, true)
    }
    pub fn new(ncommittees: usize) -> Self {
        Self::new_internal(ncommittees, false)
    }

    fn new_internal(ncommittees: usize, random_keys: bool) -> Self {
        let ictx = IntegrationTestContext::new();

        // Each committee is the payer and the matching PDA
        // The payer has money airdropped in order to init its PDA.
        // However in order to commit we can use any payer as the only
        // requirement is that the PDA is owned by its program.
        let committees = (0..ncommittees)
            .map(|_idx| {
                let payer = if random_keys {
                    Keypair::new()
                } else {
                    Keypair::from_seed(&[_idx as u8; 32]).unwrap()
                };
                ictx.airdrop_chain(&payer.pubkey(), LAMPORTS_PER_SOL)
                    .unwrap();
                let (pda, _) = pda_and_bump(&payer.pubkey());
                (payer, pda)
            })
            .collect::<Vec<(Keypair, Pubkey)>>();

        let payer = committees[0].0.insecure_clone();
        Self {
            payer,
            committees,
            common_ctx: ictx,
        }
    }

    // -----------------
    // Schedule Commit specific Transactions
    // -----------------
    pub fn init_committees(&self) -> Result<Signature> {
        let ixs = self
            .committees
            .iter()
            .map(|(payer, committee)| {
                init_account_instruction(payer.pubkey(), *committee)
            })
            .collect::<Vec<_>>();

        let payers = self
            .committees
            .iter()
            .map(|(payer, _)| payer)
            .collect::<Vec<_>>();

        // The init tx for all payers is funded by the first payer for simplicity
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&payers[0].pubkey()),
            &payers,
            self.chain_blockhash,
        );
        self.chain_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &tx,
                self.commitment,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..Default::default()
                },
            )
            .with_context(|| "Failed to initialize committees")
    }

    pub fn delegate_committees(
        &self,
        blockhash: Option<Hash>,
    ) -> Result<Signature> {
        let mut ixs = vec![];
        let mut payers = vec![];
        for (payer, _) in &self.committees {
            let ix = delegate_account_cpi_instruction(payer.pubkey());
            ixs.push(ix);
            payers.push(payer);
        }

        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&payers[0].pubkey()),
            &payers,
            blockhash.unwrap_or(self.chain_blockhash),
        );
        self.chain_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &tx,
                self.commitment,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..Default::default()
                },
            )
            .with_context(|| {
                format!(
                    "Failed to delegate committees '{:?}'",
                    tx.signatures[0]
                )
            })
    }

    // -----------------
    // Log Extractors
    // -----------------
    pub fn extract_scheduled_commit_sent_signature(
        &self,
        logs: &[String],
    ) -> Option<Signature> {
        // ScheduledCommitSent signature: <signature>
        for log in logs {
            if log.starts_with("ScheduledCommitSent signature: ") {
                let commit_sig =
                    log.split_whitespace().last().expect("No signature found");
                return Signature::from_str(commit_sig).ok();
            }
        }
        None
    }

    pub fn extract_sent_commit_info(
        &self,
        logs: &[String],
    ) -> (Vec<Pubkey>, Vec<Pubkey>, Vec<Signature>) {
        // ScheduledCommitSent included: [6ZQpzi8X2jku3C2ERgZB8hzhQ55VHLm8yZZLwTpMzHw3, 3Q49KuvoEGzGWBsbh2xgrKog66be3UM1aDEsHq7Ym4pr]
        // ScheduledCommitSent excluded: []
        // ScheduledCommitSent signature[0]: g1E7PyWZ3UHFZMJW5KqQsgoZX9PzALh4eekzjg7oGqeDPxEDfipEmV8LtTbb8EbqZfDGEaA9xbd1fADrGDGZZyi
        let mut included = vec![];
        let mut excluded = vec![];
        let mut signgatures = vec![];

        fn pubkeys_from_log_line(log: &str) -> Vec<Pubkey> {
            log.trim_end_matches(']')
                .split_whitespace()
                .skip(2)
                .flat_map(|p| {
                    let key = p
                        .trim()
                        .trim_matches(',')
                        .trim_matches('[')
                        .trim_matches(']');
                    if key.is_empty() {
                        None
                    } else {
                        Pubkey::from_str(key).ok()
                    }
                })
                .collect::<Vec<Pubkey>>()
        }

        for log in logs {
            if log.starts_with("ScheduledCommitSent included: ") {
                included = pubkeys_from_log_line(log)
            } else if log.starts_with("ScheduledCommitSent excluded: ") {
                excluded = pubkeys_from_log_line(log)
            } else if log.starts_with("ScheduledCommitSent signature[") {
                let commit_sig = log
                    .trim_end_matches(']')
                    .split_whitespace()
                    .last()
                    .and_then(|s| Signature::from_str(s).ok());
                if let Some(commit_sig) = commit_sig {
                    signgatures.push(commit_sig);
                }
            }
        }
        (included, excluded, signgatures)
    }

    pub fn extract_chain_transaction_signature(
        &self,
        logs: &[String],
    ) -> Option<Signature> {
        for log in logs {
            if log.starts_with("CommitTransactionSignature: ") {
                let commit_sig =
                    log.split_whitespace().last().expect("No signature found");
                return Signature::from_str(commit_sig).ok();
            }
        }
        None
    }

    // -----------------
    // Integration Test Context Fields
    // -----------------
    pub fn chain_client(&self) -> &RpcClient {
        &self.common_ctx.chain_client
    }
    pub fn chain_blockhash(&self) -> &Hash {
        &self.common_ctx.chain_blockhash
    }
    pub fn ephem_client(&self) -> &RpcClient {
        &self.common_ctx.ephem_client
    }
    pub fn ephem_blockhash(&self) -> &Hash {
        &self.common_ctx.ephem_blockhash
    }

    pub fn fields(&self) -> ScheduleCommitTestContextFields {
        ScheduleCommitTestContextFields {
            payer: &self.payer,
            committees: &self.committees,
            commitment: &self.commitment,
            chain_client: &self.common_ctx.chain_client,
            ephem_client: &self.common_ctx.ephem_client,
            validator_identity: &self.common_ctx.validator_identity,
            chain_blockhash: &self.common_ctx.chain_blockhash,
            ephem_blockhash: &self.common_ctx.ephem_blockhash,
        }
    }
}

// -----------------
// Integration Test Methods and Fields exposed via Deref
// -----------------
impl Deref for ScheduleCommitTestContext {
    type Target = IntegrationTestContext;

    fn deref(&self) -> &Self::Target {
        &self.common_ctx
    }
}