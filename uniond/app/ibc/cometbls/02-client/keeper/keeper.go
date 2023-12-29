package keeper

import (
	errorsmod "cosmossdk.io/errors"
	storetypes "cosmossdk.io/store/types"
	"github.com/cosmos/cosmos-sdk/codec"
	codectypes "github.com/cosmos/cosmos-sdk/codec/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/errors"
	wasmtypes "github.com/cosmos/ibc-go/modules/light-clients/08-wasm/types"
	clientkeeper "github.com/cosmos/ibc-go/v8/modules/core/02-client/keeper"
	clienttypes "github.com/cosmos/ibc-go/v8/modules/core/02-client/types" //lint:ignore SA1019 not using gov types
	connectiontypes "github.com/cosmos/ibc-go/v8/modules/core/03-connection/types"
	commitmenttypes "github.com/cosmos/ibc-go/v8/modules/core/23-commitment/types"
	"github.com/cosmos/ibc-go/v8/modules/core/exported"
)

var (
	_ exported.ClientState    = (*ClientState)(nil)
	_ exported.ConsensusState = (*ConsensusState)(nil)
)

type Keeper struct {
	cdc           codec.BinaryCodec
	clientKeeper  connectiontypes.ClientKeeper
	stakingKeeper clienttypes.StakingKeeper
}

func NewKeeper(cdc codec.BinaryCodec, clientKeeper clientkeeper.Keeper, stakingKeeper clienttypes.StakingKeeper) connectiontypes.ClientKeeper {
	return Keeper{
		cdc:           cdc,
		clientKeeper:  clientKeeper,
		stakingKeeper: stakingKeeper,
	}
}

func (k Keeper) GetClientState(ctx sdk.Context, clientID string) (exported.ClientState, bool) {
	return k.clientKeeper.GetClientState(ctx, clientID)
}

func (k Keeper) GetClientConsensusState(ctx sdk.Context, clientID string, height exported.Height) (exported.ConsensusState, bool) {
	return k.clientKeeper.GetClientConsensusState(ctx, clientID, height)
}

func (k Keeper) GetSelfConsensusState(ctx sdk.Context, height exported.Height) (exported.ConsensusState, error) {
	selfHeight, ok := height.(clienttypes.Height)
	if !ok {
		return nil, errorsmod.Wrapf(clienttypes.ErrInvalidHeight, "expected %T, got %T", clienttypes.Height{}, height)
	}
	// check that height revision matches chainID revision
	revision := clienttypes.ParseChainID(ctx.ChainID())
	if revision != height.GetRevisionNumber() {
		return nil, errorsmod.Wrapf(clienttypes.ErrInvalidHeight, "chainID revision number does not match height revision number: expected %d, got %d", revision, height.GetRevisionNumber())
	}

	histInfo, err := k.stakingKeeper.GetHistoricalInfo(ctx, int64(selfHeight.RevisionHeight))
	if err != nil {
		return nil, errorsmod.Wrapf(errors.ErrNotFound, "no historical info found at height %d", selfHeight.RevisionHeight)
	}

	timestamp := uint64(histInfo.Header.Time.Unix())

	cometblsConsensusState := &ConsensusState{
		Timestamp:          timestamp,
		Root:               commitmenttypes.NewMerkleRoot(histInfo.Header.GetAppHash()),
		NextValidatorsHash: histInfo.Header.NextValidatorsHash,
	}

	// FIXME(aeryz): we should not wrap this state in wasm since our own consensus state is just cometbls.ConsensusState
	wasmData, err := k.cdc.Marshal(cometblsConsensusState)
	if err != nil {
		return nil, errorsmod.Wrapf(err, "cannot marshal cometbls consensus state")
	}

	consensusState := &wasmtypes.ConsensusState{
		Data: wasmData,
	}

	return consensusState, nil

}

func (k Keeper) ValidateSelfClient(ctx sdk.Context, clientState exported.ClientState) error {
	// we don't have to verify cometbls client state
	return nil
}

func (k Keeper) IterateClientStates(ctx sdk.Context, prefix []byte, cb func(clientID string, cs exported.ClientState) bool) {
	k.clientKeeper.IterateClientStates(ctx, prefix, cb)
}

func (k Keeper) ClientStore(ctx sdk.Context, clientID string) storetypes.KVStore {
	return k.clientKeeper.ClientStore(ctx, clientID)
}

func (k Keeper) GetClientStatus(ctx sdk.Context, clientState exported.ClientState, clientID string) exported.Status {
	return k.clientKeeper.GetClientStatus(ctx, clientState, clientID)
}

func RegisterInterfaces(registry codectypes.InterfaceRegistry) {
	registry.RegisterImplementations(
		(*exported.ClientState)(nil),
		&ClientState{},
	)
	registry.RegisterImplementations(
		(*exported.ConsensusState)(nil),
		&ConsensusState{},
	)
}

// ===
// Cometbls exported.ClientState implementation
// ===

func (ClientState) ClientType() string { return "cometbls" }
func (cs ClientState) GetLatestHeight() exported.Height {
	return cs.LatestHeight
}
func (ClientState) Validate() error { return nil }

// Status must return the status of the client. Only Active clients are allowed to process packets.
func (ClientState) Status(_ sdk.Context, _ storetypes.KVStore, _ codec.BinaryCodec) exported.Status {
	return ""
}

// ExportMetadata must export metadata stored within the clientStore for genesis export
func (ClientState) ExportMetadata(_ storetypes.KVStore) []exported.GenesisMetadata {
	return []exported.GenesisMetadata{}
}

// ZeroCustomFields zeroes out any client customizable fields in client state
// Ledger enforced fields are maintained while all custom fields are zero values
// Used to verify upgrades
func (ClientState) ZeroCustomFields() exported.ClientState {
	return nil
}

// GetTimestampAtHeight must return the timestamp for the consensus state associated with the provided height.
func (ClientState) GetTimestampAtHeight(
	_ sdk.Context,
	_ storetypes.KVStore,
	_ codec.BinaryCodec,
	_ exported.Height,
) (uint64, error) {
	return 0, nil
}

// Initialize is called upon client creation, it allows the client to perform validation on the initial consensus state and set the
// client state, consensus state and any client-specific metadata necessary for correct light client operation in the provided client store.
func (ClientState) Initialize(_ sdk.Context, _ codec.BinaryCodec, _ storetypes.KVStore, _ exported.ConsensusState) error {
	return nil
}

// VerifyMembership is a generic proof verification method which verifies a proof of the existence of a value at a given CommitmentPath at the specified height.
// The caller is expected to construct the full CommitmentPath from a CommitmentPrefix and a standardized path (as defined in ICS 24).
func (ClientState) VerifyMembership(
	_ sdk.Context,
	_ storetypes.KVStore,
	_ codec.BinaryCodec,
	_ exported.Height,
	_ uint64,
	_ uint64,
	_ []byte,
	_ exported.Path,
	_ []byte,
) error {
	return nil
}

// VerifyNonMembership is a generic proof verification method which verifies the absence of a given CommitmentPath at a specified height.
// The caller is expected to construct the full CommitmentPath from a CommitmentPrefix and a standardized path (as defined in ICS 24).
func (ClientState) VerifyNonMembership(
	_ sdk.Context,
	_ storetypes.KVStore,
	_ codec.BinaryCodec,
	_ exported.Height,
	_ uint64,
	_ uint64,
	_ []byte,
	_ exported.Path,
) error {
	return nil
}

// VerifyClientMessage must verify a ClientMessage. A ClientMessage could be a Header, Misbehaviour, or batch update.
// It must handle each type of ClientMessage appropriately. Calls to CheckForMisbehaviour, UpdateState, and UpdateStateOnMisbehaviour
// will assume that the content of the ClientMessage has been verified and can be trusted. An error should be returned
// if the ClientMessage fails to verify.
func (ClientState) VerifyClientMessage(_ sdk.Context, _ codec.BinaryCodec, _ storetypes.KVStore, clientMsg exported.ClientMessage) error {
	return nil
}

// Checks for evidence of a misbehaviour in Header or Misbehaviour type. It assumes the ClientMessage
// has already been verified.
func (ClientState) CheckForMisbehaviour(_ sdk.Context, _ codec.BinaryCodec, _ storetypes.KVStore, _ exported.ClientMessage) bool {
	return false
}

// UpdateStateOnMisbehaviour should perform appropriate state changes on a client state given that misbehaviour has been detected and verified
func (ClientState) UpdateStateOnMisbehaviour(_ sdk.Context, _ codec.BinaryCodec, _ storetypes.KVStore, _ exported.ClientMessage) {
}

// UpdateState updates and stores as necessary any associated information for an IBC client, such as the ClientState and corresponding ConsensusState.
// Upon successful update, a list of consensus heights is returned. It assumes the ClientMessage has already been verified.
func (ClientState) UpdateState(_ sdk.Context, _ codec.BinaryCodec, _ storetypes.KVStore, _ exported.ClientMessage) []exported.Height {
	return nil
}

// CheckSubstituteAndUpdateState must verify that the provided substitute may be used to update the subject client.
// The light client must set the updated client and consensus states within the clientStore for the subject client.
func (ClientState) CheckSubstituteAndUpdateState(_ sdk.Context, _ codec.BinaryCodec, _, _ storetypes.KVStore, _ exported.ClientState) error {
	return nil
}

// Upgrade functions
// NOTE: proof heights are not included as upgrade to a new revision is expected to pass only on the last
// height committed by the current revision. Clients are responsible for ensuring that the planned last
// height of the current revision is somehow encoded in the proof verification process.
// This is to ensure that no premature upgrades occur, since upgrade plans committed to by the counterparty
// may be cancelled or modified before the last planned height.
// If the upgrade is verified, the upgraded client and consensus states must be set in the client store.
func (ClientState) VerifyUpgradeAndUpdateState(
	_ sdk.Context,
	_ codec.BinaryCodec,
	_ storetypes.KVStore,
	_ exported.ClientState,
	_ exported.ConsensusState,
	_,
	_ []byte,
) error {
	return nil
}

// ===
// Cometbls exported.ConsensusState implementation
// ===

// Consensus kind
func (ConsensusState) ClientType() string { return "cometbls" }

// GetTimestamp returns the timestamp (in nanoseconds) of the consensus state
func (cs ConsensusState) GetTimestamp() uint64 { return cs.Timestamp }

func (ConsensusState) ValidateBasic() error { return nil }
