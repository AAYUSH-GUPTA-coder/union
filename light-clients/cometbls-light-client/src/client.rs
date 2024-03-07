use std::marker::PhantomData;

use cosmwasm_std::{Deps, DepsMut, Empty, Env};
use ics008_wasm_client::{
    storage_utils::{
        read_client_state, read_consensus_state, read_subject_client_state,
        read_substitute_client_state, read_substitute_consensus_state, save_client_state,
        save_consensus_state, save_subject_client_state, save_subject_consensus_state,
    },
    IbcClient, Status, StorageState, ZERO_HEIGHT,
};
use ics23::ibc_api::SDK_SPECS;
use prost::Message;
use unionlabs::{
    encoding::Proto,
    ensure,
    hash::H256,
    ibc::{
        core::{
            client::{genesis_metadata::GenesisMetadata, height::Height},
            commitment::{
                merkle_path::MerklePath, merkle_proof::MerkleProof, merkle_root::MerkleRoot,
            },
        },
        lightclients::cometbls::{
            client_state::ClientState, consensus_state::ConsensusState, header::Header,
        },
    },
    tendermint::types::commit::Commit,
    TryFromProto,
};

use crate::{
    errors::{Error, InvalidHeaderError},
    storage::{
        get_or_next_consensus_state_meta, get_or_prev_consensus_state_meta,
        save_consensus_state_metadata,
    },
    zkp_verifier::ZKPVerifier,
};

type WasmClientState = unionlabs::ibc::lightclients::wasm::client_state::ClientState<ClientState>;
type WasmConsensusState =
    unionlabs::ibc::lightclients::wasm::consensus_state::ConsensusState<ConsensusState>;

pub struct CometblsLightClient<T: ZKPVerifier = ()>(PhantomData<T>);

impl<T: ZKPVerifier> IbcClient for CometblsLightClient<T> {
    type Error = Error;

    type CustomQuery = Empty;

    type Header = Header;

    // TODO(aeryz): Change this to appropriate misbehavior type when it is implemented
    type Misbehaviour = Header;

    type ClientState = ClientState;

    type ConsensusState = ConsensusState;

    type Encoding = Proto;

    fn verify_membership(
        deps: Deps<Self::CustomQuery>,
        height: Height,
        _delay_time_period: u64,
        _delay_block_period: u64,
        proof: Vec<u8>,
        path: MerklePath,
        value: StorageState,
    ) -> Result<(), Self::Error> {
        let consensus_state: WasmConsensusState =
            read_consensus_state(deps, &height)?.ok_or(Error::ConsensusStateNotFound(height))?;

        let merkle_proof = MerkleProof::try_from_proto_bytes(proof.as_ref()).map_err(|e| {
            Error::DecodeFromProto {
                reason: format!("{:?}", e),
            }
        })?;

        match value {
            StorageState::Occupied(value) => ics23::ibc_api::verify_membership(
                &merkle_proof,
                &SDK_SPECS,
                &consensus_state.data.app_hash,
                &path,
                value,
            ),
            StorageState::Empty => ics23::ibc_api::verify_non_membership(
                &merkle_proof,
                &SDK_SPECS,
                &consensus_state.data.app_hash,
                &path,
            ),
        }
        .map_err(Error::VerifyMembership)
    }

    fn verify_header(
        deps: Deps<Self::CustomQuery>,
        env: Env,
        header: Self::Header,
    ) -> Result<(), Self::Error> {
        let client_state: WasmClientState = read_client_state(deps)?;
        let consensus_state: WasmConsensusState =
            read_consensus_state(deps, &header.trusted_height)?
                .ok_or(Error::ConsensusStateNotFound(header.trusted_height))?;

        // SAFETY: height is bound to be 0..i64::MAX which makes it within the bounds of u64
        let untrusted_height_number = header.signed_header.commit.height.inner() as u64;
        let trusted_height_number = header.trusted_height.revision_height;

        if untrusted_height_number <= trusted_height_number {
            return Err(InvalidHeaderError::SignedHeaderHeightMustBeMoreRecent {
                signed_height: untrusted_height_number,
                trusted_height: trusted_height_number,
            }
            .into());
        }

        let trusted_timestamp = consensus_state.data.timestamp;
        let untrusted_timestamp = {
            let header_timestamp = header.signed_header.header.time.seconds.inner();
            header_timestamp
                .try_into()
                .map_err(|_| InvalidHeaderError::NegativeTimestamp(header_timestamp))?
        };

        if untrusted_timestamp <= trusted_timestamp {
            return Err(InvalidHeaderError::SignedHeaderTimestampMustBeMoreRecent {
                signed_timestamp: untrusted_timestamp,
                trusted_timestamp,
            }
            .into());
        }

        if is_client_expired(
            untrusted_timestamp,
            client_state.data.trusting_period,
            env.block.time.seconds(),
        ) {
            return Err(InvalidHeaderError::HeaderExpired(consensus_state.data.timestamp).into());
        }

        let max_clock_drift = env
            .block
            .time
            .seconds()
            .checked_add(client_state.data.max_clock_drift)
            .ok_or(Error::MathOverflow)?;

        if untrusted_timestamp >= max_clock_drift {
            return Err(InvalidHeaderError::SignedHeaderCannotExceedMaxClockDrift {
                signed_timestamp: untrusted_timestamp,
                max_clock_drift,
            }
            .into());
        }

        let trusted_validators_hash = consensus_state.data.next_validators_hash;

        let untrusted_validators_hash = if untrusted_height_number == trusted_height_number + 1 {
            &trusted_validators_hash
        } else {
            &header.signed_header.header.validators_hash
        };

        let expected_block_hash = header
            .signed_header
            .header
            .calculate_merkle_root()
            .ok_or(Error::UnableToCalculateMerkleRoot)?;

        if header.signed_header.commit.block_id.hash != expected_block_hash {
            return Err(InvalidHeaderError::SignedHeaderMismatchWithCommitHash {
                commit_hash: header.signed_header.commit.block_id.hash,
                signed_header_root: expected_block_hash,
            }
            .into());
        }

        let signed_vote = canonical_vote(
            &header.signed_header.commit,
            header.signed_header.header.chain_id.clone(),
            expected_block_hash,
        )
        .encode_length_delimited_to_vec();

        if !T::verify_zkp(
            &trusted_validators_hash.0,
            &untrusted_validators_hash.0,
            &signed_vote,
            &header.zero_knowledge_proof,
        ) {
            return Err(Error::InvalidZKP);
        }

        Ok(())
    }

    fn verify_misbehaviour(
        _deps: Deps<Self::CustomQuery>,
        _env: Env,
        _misbehaviour: Self::Misbehaviour,
    ) -> Result<(), Self::Error> {
        Err(Error::Unimplemented)
    }

    fn update_state(
        mut deps: DepsMut<Self::CustomQuery>,
        _env: Env,
        header: Self::Header,
    ) -> Result<Vec<Height>, Self::Error> {
        let mut client_state: WasmClientState = read_client_state(deps.as_ref())?;
        let mut consensus_state: WasmConsensusState =
            read_consensus_state(deps.as_ref(), &header.trusted_height)?
                .ok_or(Error::ConsensusStateNotFound(header.trusted_height))?;

        let untrusted_height = Height::new(
            header.trusted_height.revision_number,
            // SAFETY: height is bound to be 0..i64::MAX which makes it within the bounds of u64
            header.signed_header.commit.height.inner() as u64,
        );

        if untrusted_height > client_state.latest_height {
            client_state.latest_height = untrusted_height;
            client_state.data.latest_height = untrusted_height;
        }

        consensus_state.data.app_hash = MerkleRoot {
            hash: header.signed_header.header.app_hash,
        };

        consensus_state.data.next_validators_hash =
            header.signed_header.header.next_validators_hash;
        consensus_state.data.timestamp = header
            .signed_header
            .header
            .time
            .seconds
            .inner()
            .try_into()
            .map_err(|_| {
                Error::NegativeTimestamp(header.signed_header.header.time.seconds.inner())
            })?;

        save_client_state(deps.branch(), client_state);
        save_consensus_state_metadata(
            deps.branch(),
            consensus_state.data.timestamp,
            untrusted_height,
        );
        save_consensus_state(deps, consensus_state, &untrusted_height);

        Ok(vec![untrusted_height])
    }

    fn update_state_on_misbehaviour(
        _deps: DepsMut<Self::CustomQuery>,
        _env: Env,
        _client_message: Vec<u8>,
    ) -> Result<(), Self::Error> {
        Err(Error::Unimplemented)
    }

    fn check_for_misbehaviour_on_header(
        deps: Deps<Self::CustomQuery>,
        header: Self::Header,
    ) -> Result<bool, Self::Error> {
        let height = height_from_header(&header);

        let expected_timestamp: u64 = header
            .signed_header
            .header
            .time
            .seconds
            .inner()
            .try_into()
            .map_err(|_| {
                Error::NegativeTimestamp(header.signed_header.header.time.seconds.inner())
            })?;

        // If there is already a header at this height, it should be exactly the same as the header that
        // we saved previously. If this is not the case, either the client is broken or the chain is
        // broken. Because it should not be possible to have two distinct valid headers at a height.
        if let Some(WasmConsensusState {
            data:
                ConsensusState {
                    timestamp,
                    app_hash: MerkleRoot { hash },
                    next_validators_hash,
                },
        }) = read_consensus_state::<_, ConsensusState>(deps, &height)?
        {
            if timestamp != expected_timestamp
                || hash != header.signed_header.header.app_hash
                || next_validators_hash != header.signed_header.header.next_validators_hash
            {
                return Ok(true);
            }

            // We don't need to check for previous or next consensus state since we know that we already
            // saved this header correctly previously.
            return Ok(false);
        }

        if let Ok(Some((_, next_consensus_state))) = get_or_next_consensus_state_meta(deps, height)
        {
            // next (in terms of height) consensus state must have a larger timestamp
            if next_consensus_state.timestamp <= expected_timestamp {
                return Ok(true);
            }
        }

        if let Ok(Some((_, prev_consensus_state))) = get_or_prev_consensus_state_meta(deps, height)
        {
            // previous (in terms of height) consensus state must have a smaller timestamp
            if prev_consensus_state.timestamp >= expected_timestamp {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn check_for_misbehaviour_on_misbehaviour(
        _deps: Deps<Self::CustomQuery>,
        _misbehaviour: Self::Misbehaviour,
    ) -> Result<bool, Self::Error> {
        Err(Error::Unimplemented)
    }

    fn verify_upgrade_and_update_state(
        _deps: DepsMut<Self::CustomQuery>,
        _upgrade_client_state: Self::ClientState,
        _upgrade_consensus_state: Self::ConsensusState,
        _proof_upgrade_client: Vec<u8>,
        _proof_upgrade_consensus_state: Vec<u8>,
    ) -> Result<(), Self::Error> {
        Err(Error::Unimplemented)
    }

    fn migrate_client_store(mut deps: DepsMut<Self::CustomQuery>) -> Result<(), Self::Error> {
        let subject_client_state: WasmClientState = read_subject_client_state(deps.as_ref())?;
        let substitute_client_state: WasmClientState = read_substitute_client_state(deps.as_ref())?;

        ensure(
            substitute_client_state.data.frozen_height == ZERO_HEIGHT,
            Error::SubstituteClientFrozen,
        )?;

        ensure(
            migrate_check_allowed_fields(&subject_client_state.data, &substitute_client_state.data),
            Error::MigrateFieldsChanged,
        )?;

        let substitute_consensus_state: WasmConsensusState =
            read_substitute_consensus_state(deps.as_ref(), &substitute_client_state.latest_height)?
                .ok_or(Error::ConsensusStateNotFound(
                    substitute_client_state.latest_height,
                ))?;

        save_consensus_state_metadata(
            deps.branch(),
            substitute_consensus_state.data.timestamp,
            substitute_client_state.latest_height,
        );

        save_subject_consensus_state(
            deps.branch(),
            substitute_consensus_state,
            &substitute_client_state.latest_height,
        );

        let scs = substitute_client_state.data;
        save_subject_client_state(
            deps,
            WasmClientState {
                data: ClientState {
                    chain_id: scs.chain_id,
                    trusting_period: scs.trusting_period,
                    latest_height: scs.latest_height,
                    frozen_height: ZERO_HEIGHT,
                    ..subject_client_state.data
                },
                checksum: subject_client_state.checksum,
                latest_height: scs.latest_height,
            },
        );

        Ok(())
    }

    fn status(
        deps: Deps<Self::CustomQuery>,
        env: &cosmwasm_std::Env,
    ) -> Result<Status, Self::Error> {
        let client_state: WasmClientState = read_client_state(deps)?;

        if client_state.data.frozen_height != ZERO_HEIGHT {
            return Ok(Status::Frozen);
        }

        let Some(consensus_state) = read_consensus_state::<Self::CustomQuery, ConsensusState>(
            deps,
            &client_state.latest_height,
        )?
        else {
            return Ok(Status::Expired);
        };

        if is_client_expired(
            consensus_state.data.timestamp,
            client_state.data.trusting_period,
            env.block.time.seconds(),
        ) {
            return Ok(Status::Expired);
        }

        Ok(Status::Active)
    }

    fn export_metadata(
        _deps: Deps<Self::CustomQuery>,
        _env: &cosmwasm_std::Env,
    ) -> Result<Vec<GenesisMetadata>, Self::Error> {
        Ok(Vec::new())
    }

    fn timestamp_at_height(
        deps: Deps<Self::CustomQuery>,
        height: Height,
    ) -> Result<u64, Self::Error> {
        Ok(
            read_consensus_state::<Self::CustomQuery, ConsensusState>(deps, &height)?
                .ok_or(Error::ConsensusStateNotFound(height))?
                .data
                .timestamp,
        )
    }
}

fn migrate_check_allowed_fields(
    subject_client_state: &ClientState,
    substitute_client_state: &ClientState,
) -> bool {
    subject_client_state.unbonding_period == substitute_client_state.unbonding_period
        && subject_client_state.max_clock_drift == substitute_client_state.max_clock_drift
}

fn is_client_expired(
    consensus_state_timestamp: u64,
    trusting_period: u64,
    current_block_time: u64,
) -> bool {
    if let Some(sum) = consensus_state_timestamp.checked_add(trusting_period) {
        sum < current_block_time
    } else {
        // TODO(aeryz): This is really an unexpected error and this should return
        // a nice error message.
        true
    }
}

fn canonical_vote(
    commit: &Commit,
    chain_id: String,
    expected_block_hash: H256,
) -> protos::tendermint::types::CanonicalVote {
    protos::tendermint::types::CanonicalVote {
        r#type: protos::tendermint::types::SignedMsgType::Precommit as i32,
        height: commit.height.inner(),
        round: commit.round.inner() as i64,
        // TODO(aeryz): Implement BlockId to proto::CanonicalBlockId
        block_id: Some(protos::tendermint::types::CanonicalBlockId {
            hash: expected_block_hash.into(),
            part_set_header: Some(protos::tendermint::types::CanonicalPartSetHeader {
                total: commit.block_id.part_set_header.total,
                hash: commit.block_id.part_set_header.hash.0.to_vec(),
            }),
        }),
        chain_id,
    }
}

/// Returns the height from the update data
///
/// `header.signed_header.header.height` is `u64` and it does not contain the
/// revision height. This function is a utility to generate a `Height` type out
/// of the update data.
fn height_from_header(header: &Header) -> Height {
    Height {
        revision_number: header.trusted_height.revision_number,
        // SAFETY: height's bounds are [0..i64::MAX]
        revision_height: header.signed_header.header.height.inner() as u64,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cosmwasm_std::{
        testing::{mock_dependencies, MockApi, MockQuerier, MockStorage},
        OwnedDeps,
    };
    use ics008_wasm_client::{
        storage_utils::{
            consensus_db_key, read_subject_consensus_state, HOST_CLIENT_STATE_KEY,
            SUBJECT_CLIENT_STORE_PREFIX, SUBSTITUTE_CLIENT_STORE_PREFIX,
        },
        FROZEN_HEIGHT,
    };
    use unionlabs::{google::protobuf::any::Any, IntoProto};

    use super::*;

    const INITIAL_CONSENSUS_STATE_HEIGHT: Height = Height {
        revision_number: 1,
        revision_height: 1124,
    };

    const INITIAL_SUBSTITUTE_CONSENSUS_STATE_HEIGHT: Height = Height {
        revision_number: 1,
        revision_height: 1200,
    };

    fn save_states_to_migrate_store(
        deps: DepsMut,
        subject_client_state: &WasmClientState,
        substitute_client_state: &WasmClientState,
        subject_consensus_state: &WasmConsensusState,
        substitute_consensus_state: &WasmConsensusState,
    ) {
        deps.storage.set(
            format!("{SUBJECT_CLIENT_STORE_PREFIX}{HOST_CLIENT_STATE_KEY}").as_bytes(),
            &Any(subject_client_state.clone()).into_proto_bytes(),
        );
        deps.storage.set(
            format!(
                "{SUBJECT_CLIENT_STORE_PREFIX}{}",
                consensus_db_key(&INITIAL_CONSENSUS_STATE_HEIGHT)
            )
            .as_bytes(),
            &Any(subject_consensus_state.clone()).into_proto_bytes(),
        );
        deps.storage.set(
            format!("{SUBSTITUTE_CLIENT_STORE_PREFIX}{HOST_CLIENT_STATE_KEY}").as_bytes(),
            &Any(substitute_client_state.clone()).into_proto_bytes(),
        );
        deps.storage.set(
            format!(
                "{SUBSTITUTE_CLIENT_STORE_PREFIX}{}",
                consensus_db_key(&INITIAL_SUBSTITUTE_CONSENSUS_STATE_HEIGHT)
            )
            .as_bytes(),
            &Any(substitute_consensus_state.clone()).into_proto_bytes(),
        );
    }

    fn prepare_migrate_tests() -> (
        OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
        WasmClientState,
        WasmConsensusState,
        WasmClientState,
        WasmConsensusState,
    ) {
        (
            mock_dependencies(),
            serde_json::from_str(&fs::read_to_string("src/test/client_state.json").unwrap())
                .unwrap(),
            serde_json::from_str(&fs::read_to_string("src/test/consensus_state.json").unwrap())
                .unwrap(),
            serde_json::from_str(
                &fs::read_to_string("src/test/substitute_client_state.json").unwrap(),
            )
            .unwrap(),
            serde_json::from_str(
                &fs::read_to_string("src/test/substitute_consensus_state.json").unwrap(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn migrate_client_store_works() {
        let (
            mut deps,
            mut wasm_client_state,
            wasm_consensus_state,
            substitute_wasm_client_state,
            substitute_wasm_consensus_state,
        ) = prepare_migrate_tests();

        wasm_client_state.data.frozen_height = FROZEN_HEIGHT;

        save_states_to_migrate_store(
            deps.as_mut(),
            &wasm_client_state,
            &substitute_wasm_client_state,
            &wasm_consensus_state,
            &substitute_wasm_consensus_state,
        );

        CometblsLightClient::<()>::migrate_client_store(deps.as_mut()).unwrap();

        let wasm_client_state: WasmClientState = read_subject_client_state(deps.as_ref()).unwrap();
        // we didn't miss updating any fields
        assert_eq!(wasm_client_state, substitute_wasm_client_state);
        // client is unfrozen
        assert_eq!(wasm_client_state.data.frozen_height, ZERO_HEIGHT);

        // the new consensus state is saved under the correct height
        assert_eq!(
            read_subject_consensus_state(deps.as_ref(), &INITIAL_SUBSTITUTE_CONSENSUS_STATE_HEIGHT)
                .unwrap()
                .unwrap(),
            substitute_wasm_consensus_state
        );

        // the new consensus state metadata is saved under substitute's latest height
        assert_eq!(
            get_or_next_consensus_state_meta(
                deps.as_ref(),
                INITIAL_SUBSTITUTE_CONSENSUS_STATE_HEIGHT
            )
            .unwrap()
            .unwrap()
            .0,
            substitute_wasm_client_state.latest_height
        );
    }

    #[test]
    fn migrate_client_store_fails_when_invalid_change() {
        let (
            mut deps,
            wasm_client_state,
            wasm_consensus_state,
            substitute_wasm_client_state,
            substitute_wasm_consensus_state,
        ) = prepare_migrate_tests();

        macro_rules! modify_fns {
            ($param:ident, $($m:expr), + $(,)?) => ([$(|$param: &mut ClientState| $m),+])
        }

        let modifications = modify_fns! { s,
            s.unbonding_period ^= u64::MAX,
            s.max_clock_drift ^= u64::MAX,
        };

        for m in modifications {
            let mut state = substitute_wasm_client_state.clone();
            m(&mut state.data);

            save_states_to_migrate_store(
                deps.as_mut(),
                &wasm_client_state,
                &state,
                &wasm_consensus_state,
                &substitute_wasm_consensus_state,
            );
            assert_eq!(
                CometblsLightClient::<()>::migrate_client_store(deps.as_mut()),
                Err(Error::MigrateFieldsChanged)
            );
        }
    }

    #[test]
    fn migrate_client_store_fails_when_substitute_client_frozen() {
        let (
            mut deps,
            wasm_client_state,
            wasm_consensus_state,
            mut substitute_wasm_client_state,
            substitute_wasm_consensus_state,
        ) = prepare_migrate_tests();

        substitute_wasm_client_state.data.frozen_height = FROZEN_HEIGHT;

        save_states_to_migrate_store(
            deps.as_mut(),
            &wasm_client_state,
            &substitute_wasm_client_state,
            &wasm_consensus_state,
            &substitute_wasm_consensus_state,
        );

        assert_eq!(
            CometblsLightClient::<()>::migrate_client_store(deps.as_mut()),
            Err(Error::SubstituteClientFrozen)
        );
    }
}
