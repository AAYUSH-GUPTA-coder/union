use std::collections::BTreeMap;

use unionlabs::{
    google::protobuf::{duration::Duration, timestamp::Timestamp},
    hash::{H160, H512},
    ibc::lightclients::tendermint::fraction::Fraction,
    tendermint::{
        crypto::public_key::PublicKey,
        types::{
            block_id::BlockId, commit::Commit, commit_sig::CommitSig, signed_header::SignedHeader,
            validator_set::ValidatorSet,
        },
    },
};

use crate::{
    error::Error,
    types::{BatchSignatureVerifier, SignatureVerifier},
    utils::{canonical_vote, get_validator_by_address, header_expired, validators_hash},
};

pub fn verify<V: SignatureVerifier, B: BatchSignatureVerifier>(
    trusted_header: &SignedHeader,
    trusted_vals: &ValidatorSet,     // height=X or height=X+1
    untrusted_header: &SignedHeader, // height=Y
    untrusted_vals: &ValidatorSet,   // height=Y
    trusting_period: Duration,
    now: Timestamp,
    max_clock_drift: Duration,
    trust_level: Fraction,
) -> Result<(), Error> {
    // check adjacency in terms of block(header) height
    if untrusted_header.header.height.inner()
        != trusted_header
            .header
            .height
            .inner()
            .checked_add(1)
            .ok_or(Error::IntegerOverflow)?
    {
        verify_non_adjacent::<V, B>(
            trusted_header,
            trusted_vals,
            untrusted_header,
            untrusted_vals,
            trusting_period,
            now,
            max_clock_drift,
            trust_level,
        )
    } else {
        verify_adjacent::<V, B>(
            trusted_header,
            untrusted_header,
            untrusted_vals,
            trusting_period,
            now,
            max_clock_drift,
        )
    }
}

// TODO(aeryz): Official docs, change
/// verifies non-adjacent untrustedHeader against
/// trustedHeader. It ensures that:
///
///	1. trustedHeader can still be trusted
///	2. untrustedHeader is valid
///	3. trustLevel ([1/3, 1]) of trustedHeaderVals signed correctly
///	4. more than 2/3 of untrustedVals have signed h2
///	5. headers are non-adjacent.
///
/// maxClockDrift defines how much untrustedHeader.Time can drift into the
/// future.
pub fn verify_non_adjacent<V: SignatureVerifier, B: BatchSignatureVerifier>(
    trusted_header: &SignedHeader,
    trusted_vals: &ValidatorSet,     // height=X or height=X+1
    untrusted_header: &SignedHeader, // height=Y
    untrusted_vals: &ValidatorSet,   // height=Y
    trusting_period: Duration,
    now: Timestamp,
    max_clock_drift: Duration,
    trust_level: Fraction,
) -> Result<(), Error> {
    // We only want this check to be done when the headers are not adjacent
    if untrusted_header.header.height.inner()
        == trusted_header
            .header
            .height
            .inner()
            .checked_add(1)
            .ok_or(Error::IntegerOverflow)?
    {
        return Err(Error::HeadersMustBeNonAdjacent);
    }

    if header_expired(trusted_header, trusting_period, now) {
        return Err(Error::HeaderExpired {
            trusting_period,
            header_timestamp: trusted_header.header.time,
        });
    }

    verify_new_headers_and_vals(
        untrusted_header,
        untrusted_vals,
        trusted_header,
        now,
        max_clock_drift,
    )?;

    verify_commit_light_trusting::<V, B>(
        &trusted_header.header.chain_id,
        trusted_vals,
        &untrusted_header.commit,
        trust_level,
    )?;

    verify_commit_light::<V, B>(
        untrusted_vals,
        &trusted_header.header.chain_id,
        &untrusted_header.commit.block_id,
        untrusted_header.header.height.inner(),
        &untrusted_header.commit,
    )?;

    Ok(())
}

pub fn verify_adjacent<V: SignatureVerifier, B: BatchSignatureVerifier>(
    trusted_header: &SignedHeader,
    untrusted_header: &SignedHeader, // height=Y
    untrusted_vals: &ValidatorSet,   // height=Y
    trusting_period: Duration,
    now: Timestamp,
    max_clock_drift: Duration,
) -> Result<(), Error> {
    if untrusted_header.header.height.inner()
        != trusted_header
            .header
            .height
            .inner()
            .checked_add(1)
            .ok_or(Error::IntegerOverflow)?
    {
        return Err(Error::HeadersMustBeAdjacent);
    }

    if header_expired(trusted_header, trusting_period, now) {
        return Err(Error::HeaderExpired {
            trusting_period,
            header_timestamp: trusted_header.header.time,
        });
    }

    verify_new_headers_and_vals(
        untrusted_header,
        untrusted_vals,
        trusted_header,
        now,
        max_clock_drift,
    )?;

    if untrusted_header.header.validators_hash != trusted_header.header.next_validators_hash {
        return Err(Error::NextValidatorsHashMismatch {
            next_validators_hash: untrusted_header.header.next_validators_hash.clone(),
            validators_hash: trusted_header.header.next_validators_hash.clone(),
        });
    }

    verify_commit_light::<V, B>(
        untrusted_vals,
        &trusted_header.header.chain_id,
        &untrusted_header.commit.block_id,
        untrusted_header.header.height.inner(),
        &untrusted_header.commit,
    )?;

    Ok(())
}

pub fn verify_commit_light<V: SignatureVerifier, B: BatchSignatureVerifier>(
    vals: &ValidatorSet,
    chain_id: &str,
    block_id: &BlockId,
    height: i64,
    commit: &Commit,
) -> Result<(), Error> {
    verify_basic_vals_and_commit(vals, commit, height, block_id)?;

    let voting_power_needed = TryInto::<u64>::try_into(vals.total_voting_power)
        .map_err(|_| Error::NegativeVotingPower(vals.total_voting_power))?
        * 2
        / 3;

    let filter_commit = |commit_sig: &CommitSig| -> Option<(H160, Timestamp, H512)> {
        match commit_sig {
            CommitSig::Commit {
                validator_address,
                timestamp,
                signature,
            } => Some((
                validator_address.clone(),
                timestamp.clone(),
                signature.clone(),
            )),
            _ => None,
        }
    };

    if B::should_batch_verify(commit.signatures.len()) {
        verify_commit_batch::<B>(
            chain_id,
            vals,
            commit,
            voting_power_needed,
            filter_commit,
            true,
        )
    } else {
        verify_commit_single::<V>(
            chain_id,
            vals,
            commit,
            voting_power_needed,
            filter_commit,
            true,
        )
    }
}

fn verify_basic_vals_and_commit(
    vals: &ValidatorSet,
    commit: &Commit,
    height: i64,
    block_id: &BlockId,
) -> Result<(), Error> {
    if vals.validators.len() != commit.signatures.len() {
        return Err(Error::InvalidCommitSignaturesLength {
            sig_len: commit.signatures.len(),
            val_len: vals.validators.len(),
        });
    }

    if height != commit.height.inner() {
        return Err(Error::InvalidCommitHeight {
            commit_height: commit.height.inner(),
            height,
        });
    }

    if block_id != &commit.block_id {
        return Err(Error::InvalidCommitBlockId {
            commit_block_id: commit.block_id.clone(),
            block_id: block_id.clone(),
        });
    }

    Ok(())
}

pub fn verify_commit_light_trusting<V: SignatureVerifier, B: BatchSignatureVerifier>(
    chain_id: &str,
    vals: &ValidatorSet,
    commit: &Commit,
    trust_level: Fraction,
) -> Result<(), Error> {
    // TODO(aeryz): cometbft recalculates it if this is 0, why?
    // SAFETY: as u64 is safe here since we do `abs` which makes it always positive
    let total_voting_power_mul_by_numerator = (vals.total_voting_power.abs() as u64)
        .checked_mul(trust_level.numerator)
        .ok_or(Error::IntegerOverflow)?;
    let voting_power_needed = total_voting_power_mul_by_numerator
        .checked_div(trust_level.denominator)
        .ok_or(Error::TrustLevelZeroDenominator)?;

    // only use the commit signatures
    let filter_commit = |commit_sig: &CommitSig| -> Option<(H160, Timestamp, H512)> {
        match commit_sig {
            CommitSig::Commit {
                validator_address,
                timestamp,
                signature,
            } => Some((
                validator_address.clone(),
                timestamp.clone(),
                signature.clone(),
            )),
            _ => None,
        }
    };

    // attempt to batch verify commit. As the validator set doesn't necessarily
    // correspond with the validator set that signed the block we need to look
    // up by address rather than index.
    if B::should_batch_verify(commit.signatures.len()) {
        verify_commit_batch::<B>(
            chain_id,
            vals,
            commit,
            voting_power_needed,
            filter_commit,
            false,
        )
    } else {
        verify_commit_single::<V>(
            chain_id,
            vals,
            commit,
            voting_power_needed,
            filter_commit,
            false,
        )
    }
}

fn verify_commit_single<V: SignatureVerifier>(
    chain_id: &str,
    vals: &ValidatorSet,
    commit: &Commit,
    voting_power_needed: u64,
    filter_commit: fn(&CommitSig) -> Option<(H160, Timestamp, H512)>,
    lookup_by_index: bool,
) -> Result<(), Error> {
    verify_commit(
        chain_id,
        vals,
        commit,
        voting_power_needed,
        filter_commit,
        lookup_by_index,
        |pubkey, msg, signature| {
            if V::verify_signature(pubkey, &msg, signature) {
                Ok(())
            } else {
                Err(Error::SignatureVerification)
            }
        },
    )
}

fn verify_commit_batch<V: BatchSignatureVerifier>(
    chain_id: &str,
    vals: &ValidatorSet,
    commit: &Commit,
    voting_power_needed: u64,
    filter_commit: fn(&CommitSig) -> Option<(H160, Timestamp, H512)>,
    lookup_by_index: bool,
) -> Result<(), Error> {
    let mut batch_verifier = V::new();
    verify_commit(
        chain_id,
        vals,
        commit,
        voting_power_needed,
        filter_commit,
        lookup_by_index,
        move |pubkey, msg, signature| {
            batch_verifier
                .add(pubkey, msg, signature)
                .map_err(|e| Error::BatchVerification(Box::new(e)))
        },
    )
}

fn verify_commit<F: FnMut(&PublicKey, Vec<u8>, &[u8]) -> Result<(), Error>>(
    chain_id: &str,
    vals: &ValidatorSet,
    commit: &Commit,
    voting_power_needed: u64,
    filter_commit: fn(&CommitSig) -> Option<(H160, Timestamp, H512)>,
    lookup_by_index: bool,
    mut signature_handle: F,
) -> Result<(), Error> {
    let mut tallied_voting_power: u64 = 0;
    let mut seen_vals: BTreeMap<usize, usize> = BTreeMap::new();

    for (i, commit_sig) in commit.signatures.iter().enumerate() {
        let Some((validator_address, timestamp, signature)) = filter_commit(commit_sig) else {
            continue;
        };

        let val = if lookup_by_index {
            vals.validators
                .get(i)
                .ok_or(Error::InvalidIndexInValidatorSet {
                    index: i,
                    val_len: vals.validators.len(),
                })?
        } else {
            let Some((val_idx, val)) = get_validator_by_address(vals, &validator_address) else {
                continue;
            };

            // `insert` returns the value if there already exists a value
            if seen_vals.insert(val_idx, i).is_some() {
                return Err(Error::DoubleVote(validator_address));
            }

            val
        };

        let vote_sign_bytes = canonical_vote(commit, commit_sig, &timestamp, chain_id);

        signature_handle(&val.pub_key, vote_sign_bytes, signature.as_ref())?;

        // If this signature counts then add the voting power of the validator
        // to the tally
        tallied_voting_power += val.voting_power.inner() as u64; // SAFE because within the bounds

        if tallied_voting_power > voting_power_needed {
            break;
        }
    }

    if tallied_voting_power <= voting_power_needed {
        Err(Error::NotEnoughVotingPower {
            have: tallied_voting_power,
            need: voting_power_needed,
        })
    } else {
        Ok(())
    }
}

fn verify_new_headers_and_vals(
    untrusted_header: &SignedHeader, // height=Y
    untrusted_vals: &ValidatorSet,   // height=Y
    trusted_header: &SignedHeader,
    now: Timestamp,
    max_clock_drift: Duration,
) -> Result<(), Error> {
    // SH HEADER VALIDATE BASIC
    // TODO(aeryz): move these untrusted_header.validate_basic related checks to elsewhere, this function gets too bloated
    if untrusted_header.commit.height != untrusted_header.header.height {
        return Err(Error::SignedHeaderCommitHeightMismatch {
            sh_height: untrusted_header.header.height.inner(),
            commit_height: untrusted_header.commit.height.inner(),
        });
    }

    let untrusted_header_hash = untrusted_header
        .header
        .calculate_merkle_root()
        .ok_or(Error::InvalidHeader)?;
    if untrusted_header_hash != untrusted_header.commit.block_id.hash {
        return Err(Error::SignedHeaderCommitHashMismatch {
            sh_hash: untrusted_header_hash,
            commit_hash: untrusted_header.commit.block_id.hash.clone(),
        });
    }

    if untrusted_header.header.chain_id != trusted_header.header.chain_id {
        return Err(Error::ChainIdMismatch {
            untrusted_header_chain_id: untrusted_header.header.chain_id.clone(),
            trusted_header_chain_id: trusted_header.header.chain_id.clone(),
        });
    }
    // SH HEADER VALIDATE BASIC END

    // we can only update using a latter header
    if untrusted_header.header.height <= trusted_header.header.height {
        return Err(Error::UntrustedHeaderHeightIsSmaller {
            untrusted_header_height: untrusted_header.header.height.inner(),
            trusted_header_height: trusted_header.header.height.inner(),
        });
    }

    // a header with a greater height can never have <= time
    if untrusted_header.header.time <= trusted_header.header.time {
        return Err(Error::UntrustedHeaderTimestampIsSmaller {
            untrusted_header_timestamp: untrusted_header.header.time,
            trusted_header_timestamp: trusted_header.header.time,
        });
    }

    let drift_timestamp =
        now.checked_add(max_clock_drift)
            .ok_or(Error::MaxClockDriftCheckFailed {
                max_clock_drift,
                timestamp: now,
            })?;

    if untrusted_header.header.time >= drift_timestamp {
        return Err(Error::MaxClockDriftCheckFailed {
            max_clock_drift,
            timestamp: untrusted_header.header.time,
        });
    }

    if untrusted_header.header.validators_hash != validators_hash(untrusted_vals) {
        return Err(Error::UntrustedValidatorSetMismatch);
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    use unionlabs::{
        ibc::lightclients::tendermint::header::Header, tendermint::crypto::public_key::PublicKey,
    };

    use super::*;
    use crate::types::BatchVerificationError;

    struct EdVerifier;

    impl SignatureVerifier for EdVerifier {
        fn verify_signature(pubkey: &PublicKey, msg: &[u8], sig: &[u8]) -> bool {
            let PublicKey::Ed25519(pubkey) = pubkey else {
                panic!("invalid pubkey");
            };
            let key: VerifyingKey =
                VerifyingKey::from_bytes(pubkey.as_slice().try_into().unwrap()).unwrap();
            let signature: Signature = Signature::from_bytes(sig.try_into().unwrap());
            key.verify(msg, &signature).is_ok()
        }
    }

    #[derive(Default)]
    struct BatchEdVerifier {
        signatures: Vec<Signature>,
        messages: Vec<Vec<u8>>,
        verifying_keys: Vec<VerifyingKey>,
    }

    impl BatchSignatureVerifier for BatchEdVerifier {
        type Error = BatchVerificationError;

        fn should_batch_verify(signature_len: usize) -> bool {
            signature_len >= 2
        }

        fn new() -> Self {
            BatchEdVerifier::default()
        }

        fn add(
            &mut self,
            pubkey: &PublicKey,
            msg: Vec<u8>,
            signature: &[u8],
        ) -> Result<(), Self::Error> {
            let PublicKey::Ed25519(pubkey) = pubkey else {
                panic!("invalid pubkey");
            };
            let key: VerifyingKey =
                VerifyingKey::from_bytes(pubkey.as_slice().try_into().unwrap()).unwrap();
            let signature: Signature = Signature::from_bytes(signature.try_into().unwrap());

            self.signatures.push(signature);
            self.verifying_keys.push(key);
            self.messages.push(msg.into());

            Ok(())
        }

        fn verify_signature(&self) -> bool {
            ed25519_dalek::verify_batch(
                self.messages
                    .iter()
                    .map(|v| v.as_slice())
                    .collect::<Vec<_>>()
                    .as_slice(),
                &self.signatures,
                &self.verifying_keys,
            )
            .is_ok()
        }
    }

    #[test]
    fn verify_works() {
        let initial_header: Header = serde_json::from_str(include_str!("test/288.json")).unwrap();
        let update_header: Header = serde_json::from_str(include_str!("test/291.json")).unwrap();

        verify::<EdVerifier, ()>(
            &initial_header.signed_header,
            &initial_header.validator_set,
            &update_header.signed_header,
            &update_header.validator_set,
            Duration::new(315576000000, 0).unwrap(),
            update_header.signed_header.header.time,
            Duration::new(100_000_000, 0).unwrap(),
            Fraction {
                numerator: 1,
                denominator: 3,
            },
        )
        .unwrap();
    }

    #[test]
    fn batch_verify_works() {
        let initial_header: Header = serde_json::from_str(include_str!("test/288.json")).unwrap();
        let update_header: Header = serde_json::from_str(include_str!("test/291.json")).unwrap();

        verify::<EdVerifier, BatchEdVerifier>(
            &initial_header.signed_header,
            &initial_header.validator_set,
            &update_header.signed_header,
            &update_header.validator_set,
            Duration::new(315576000000, 0).unwrap(),
            update_header.signed_header.header.time,
            Duration::new(100_000_000, 0).unwrap(),
            Fraction {
                numerator: 1,
                denominator: 3,
            },
        )
        .unwrap();
    }
}
