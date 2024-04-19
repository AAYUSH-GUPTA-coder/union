use cosmwasm_std::{entry_point, DepsMut, Env, MessageInfo, Response};
use ics008_wasm_client::{
    define_cosmwasm_light_client_contract,
    storage_utils::{save_proto_client_state, save_proto_consensus_state},
    InstantiateMsg,
};
use protos::ibc::lightclients::wasm::v1::{
    ClientState as ProtoClientState, ConsensusState as ProtoConsensusState,
};
use unionlabs::{
    encoding::{DecodeAs, Proto},
    ibc::{core::client::height::Height, lightclients::ethereum::client_state::ClientState},
};

use crate::{
    client::{validate_checkpoint_root_index, EthereumLightClient},
    errors::Error,
};

// NOTE(aeryz): the fact that the host module forces the light clients to store and use the wasm wrapping
// in the client state makes this code kinda messy. But this is going to be resolved in the future versions
// of IBC (probably v9). When that feature is implemented, we can move this to the ics008 macro.
#[entry_point]
pub fn instantiate(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, Error> {
    let client_state =
        ClientState::decode_as::<Proto>(&msg.client_state).map_err(|e| Error::DecodeFromProto {
            reason: format!("{:?}", e),
        })?;

    validate_checkpoint_root_index(client_state.checkpoint_root_index)?;

    save_proto_consensus_state(
        deps.branch(),
        ProtoConsensusState {
            data: msg.consensus_state.into(),
        },
        &Height {
            revision_number: 0,
            revision_height: client_state.latest_slot,
        },
    );
    save_proto_client_state(
        deps,
        ProtoClientState {
            data: msg.client_state.into(),
            checksum: msg.checksum.into(),
            latest_height: Some(
                Height {
                    revision_number: 0,
                    revision_height: client_state.latest_slot,
                }
                .into(),
            ),
        },
    );
    Ok(Response::default())
}

#[cfg(feature = "mainnet")]
define_cosmwasm_light_client_contract!(EthereumLightClient, EthereumMainnet);
#[cfg(feature = "minimal")]
define_cosmwasm_light_client_contract!(EthereumLightClient, EthereumMinimal);
