pragma solidity ^0.8.18;

import "forge-std/Test.sol";
import {IBCMsgs} from "contracts/core/25-handler/IBCMsgs.sol";
import {
    IbcLightclientsMockV1ClientState as MockClientState,
    IbcLightclientsMockV1Header as MockHeader,
    IbcLightclientsMockV1ConsensusState as MockConsensusState,
    IbcCoreClientV1Height as ClientHeight
} from "contracts/proto/MockClient.sol";
import {GoogleProtobufAny as Any} from "contracts/proto/GoogleProtobufAny.sol";
import {
    IbcCoreChannelV1Counterparty as ChannelCounterparty,
    IbcCoreChannelV1Channel as Channel,
    IbcCoreChannelV1GlobalEnums as ChannelEnums,
    IbcCoreChannelV1Counterparty as ChannelCounterparty
} from "contracts/proto/ibc/core/channel/v1/channel.sol";
import {
    IbcCoreConnectionV1Counterparty as ConnectionCounterparty,
    IbcCoreConnectionV1Version as ConnectionVersion,
    IbcCoreConnectionV1ConnectionEnd as ConnectionEnd,
    IbcCoreConnectionV1GlobalEnums as ConnectionEnums
} from "contracts/proto/ibc/core/connection/v1/connection.sol";
import {IbcCoreCommitmentV1MerklePrefix as CommitmentMerklePrefix} from
    "contracts/proto/ibc/core/commitment/v1/commitment.sol";

library MsgMocks {
    //
    // IBCClient msgs
    //

    /// Builds a MsgCreateClient
    /// TODO: should we fuzz over height.revision_number?
    function createClient(string memory clientType, uint64 revisionHeight)
        internal
        view
        returns (IBCMsgs.MsgCreateClient memory m)
    {
        m.clientType = clientType;
        m.clientStateBytes = wrapAnyMockClientState(
            MockClientState.Data({
                latest_height: ClientHeight.Data({revision_number: 0, revision_height: revisionHeight})
            })
        );
        m.consensusStateBytes = wrapAnyMockConsensusState(MockConsensusState.Data({timestamp: uint64(block.timestamp)}));
    }

    /// Builds a MsgUpdateClient
    /// TODO: should we fuzz over height.revision_number?
    function updateClient(string memory clientId, uint64 nextRevisionHeight)
        internal
        view
        returns (IBCMsgs.MsgUpdateClient memory m)
    {
        m.clientId = clientId;
        m.clientMessage = wrapAnyMockHeader(
            MockHeader.Data({
                height: ClientHeight.Data({revision_number: 0, revision_height: nextRevisionHeight}),
                timestamp: uint64(block.timestamp)
            })
        );
    }

    //
    // IBCConnection msgs
    //

    /// Builds a MsgConnectionOpenInit
    function connectionOpenInit(string memory clientId)
        internal
        view
        returns (IBCMsgs.MsgConnectionOpenInit memory m)
    {
        m.clientId = clientId;
        m.counterparty.client_id = "counterparty-client-id";
        m.counterparty.connection_id = "counterparty-conn-id";
    }

    /// Builds a MsgConnectionOpenTry
    /// TODO: should we fuzz over version.identifier?
    function connectionOpenTry(string memory clientId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgConnectionOpenTry memory m)
    {
        m.clientId = clientId;
        m.counterparty.client_id = "counterparty-client-id";
        m.counterparty.connection_id = "counterparty-conn-id";
        m.counterpartyVersions = new ConnectionVersion.Data[](1);
        m.counterpartyVersions[0] = ConnectionVersion.Data({identifier: "1", features: new string[](0)});

        // mocking connection data
        ConnectionEnd.Data memory connection = ConnectionEnd.Data({
            client_id: "counterparty-client-id",
            versions: m.counterpartyVersions,
            state: ConnectionEnums.State.STATE_INIT,
            delay_period: 0,
            counterparty: ConnectionCounterparty.Data({
                client_id: clientId,
                connection_id: "",
                prefix: CommitmentMerklePrefix.Data({key_prefix: bytes(commitment_prefix())})
            })
        });

        bytes memory encodedConnection = ConnectionEnd.encode(connection);
        m.proofInit = abi.encodePacked(sha256(encodedConnection));

        // for MockClient, it seems this value doesn't matter
        // it just checks sha256(clientStateBytes) == proofClient
        m.clientStateBytes = abi.encodePacked(bytes32(uint256(0x1)));
        m.proofClient = abi.encodePacked(sha256(m.clientStateBytes));
        m.proofHeight.revision_height = proofHeight;
    }

    /// Builds a MsgConnectionOpenAck
    /// TODO: what should msg.clientStateBytes be?
    //. TODO: msg.counterpartyConnectionId is ignored by MockClient. but probably should be set for CometblsClient
    /// TODO: what other fields should we fill here?
    function connectionOpenAck(string memory clientId, string memory connId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgConnectionOpenAck memory m)
    {
        m.connectionId = connId;
        m.version = ConnectionVersion.Data({identifier: "1", features: new string[](0)});
        m.counterpartyConnectionID = "counterparty-conn-id";

        // mocking connection data
        ConnectionEnd.Data memory connection = ConnectionEnd.Data({
            client_id: "counterparty-client-id",
            versions: new ConnectionVersion.Data[](1),
            state: ConnectionEnums.State.STATE_TRYOPEN,
            delay_period: 0,
            counterparty: ConnectionCounterparty.Data({
                client_id: clientId,
                connection_id: connId,
                prefix: CommitmentMerklePrefix.Data({key_prefix: bytes(commitment_prefix())})
            })
        });
        connection.versions[0] = m.version;

        bytes memory encodedConnection = ConnectionEnd.encode(connection);

        // for MockClient, it seems this value doesn't matter
        // it just checks sha256(clientStateBytes) == proofClient
        m.clientStateBytes = abi.encodePacked(bytes32(uint256(0x1)));
        m.proofClient = abi.encodePacked(sha256(m.clientStateBytes));
        m.proofTry = abi.encodePacked(sha256(encodedConnection));
        m.proofHeight.revision_height = proofHeight;
    }

    /// Builds a MsgConnectionOpenConfirm
    function connectionOpenConfirm(string memory clientId, string memory connId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgConnectionOpenConfirm memory m)
    {
        m.connectionId = connId;

        // mocking connection data
        ConnectionEnd.Data memory connection = ConnectionEnd.Data({
            client_id: "counterparty-client-id",
            versions: new ConnectionVersion.Data[](1),
            state: ConnectionEnums.State.STATE_OPEN,
            delay_period: 0,
            counterparty: ConnectionCounterparty.Data({
                client_id: clientId,
                connection_id: connId,
                prefix: CommitmentMerklePrefix.Data({key_prefix: bytes(commitment_prefix())})
            })
        });
        connection.versions[0] = ConnectionVersion.Data({identifier: "1", features: new string[](2)});
        connection.versions[0].features[0] = "ORDER_ORDERED";
        connection.versions[0].features[1] = "ORDER_UNORDERED";

        bytes memory encodedConnection = ConnectionEnd.encode(connection);
        m.proofAck = abi.encodePacked(sha256(encodedConnection));
        m.proofHeight.revision_height = proofHeight;
    }

    /// Builds a MsgChannelOpenInit
    function channelOpenInit(string memory connId, string memory portId)
        internal
        view
        returns (IBCMsgs.MsgChannelOpenInit memory m)
    {
        ChannelCounterparty.Data memory counterparty;
        counterparty.port_id = "counterparty-port-id";
        counterparty.channel_id = "counterparty-channel-id";
        string[] memory hops = new string[](1);
        hops[0] = connId;

        m.channel.state = ChannelEnums.State.STATE_INIT;
        m.channel.counterparty = counterparty;
        m.channel.connection_hops = hops;
        // TODO: apparently channel creation works without setting this field, but later recvPacket fails because channel ordering is unspecified (IBCPacket.sol:138)
        // is this a bug or some behaviour I'm not understanding?
        m.channel.ordering = ChannelEnums.Order.ORDER_ORDERED;
        m.portId = portId;
    }

    function channelOpenAck(string memory portId, string memory channelId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgChannelOpenAck memory m)
    {
        m.portId = portId;
        m.channelId = channelId;
        m.counterpartyVersion = "counterparty-version";
        m.counterpartyChannelId = "counterparty-channel-id";

        // mocking channel data
        Channel.Data memory channel = Channel.Data({
            state: ChannelEnums.State.STATE_TRYOPEN,
            ordering: ChannelEnums.Order.ORDER_ORDERED,
            counterparty: ChannelCounterparty.Data({port_id: portId, channel_id: channelId}),
            connection_hops: new string[](1),
            version: m.counterpartyVersion
        });
        channel.connection_hops[0] = "counterparty-conn-id";

        bytes memory encodedChannel = Channel.encode(channel);
        m.proofTry = abi.encodePacked(sha256(encodedChannel));
        m.proofHeight.revision_height = proofHeight;
    }

    function commitment_prefix() internal pure returns (string memory) {
        return "ibc";
    }

    function channelOpenTry(string memory connId, string memory portId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgChannelOpenTry memory m)
    {
        m.portId = portId;
        m.counterpartyVersion = "counterparty-version";
        m.channel = Channel.Data({
            state: ChannelEnums.State.STATE_TRYOPEN,
            ordering: ChannelEnums.Order.ORDER_NONE_UNSPECIFIED,
            counterparty: ChannelCounterparty.Data({port_id: portId, channel_id: ""}),
            connection_hops: new string[](1),
            version: m.counterpartyVersion
        });
        m.channel.connection_hops[0] = connId;

        // expected channel
        Channel.Data memory expectedChannel = Channel.Data({
            state: ChannelEnums.State.STATE_INIT,
            ordering: ChannelEnums.Order.ORDER_NONE_UNSPECIFIED,
            counterparty: ChannelCounterparty.Data({port_id: portId, channel_id: ""}),
            connection_hops: new string[](1),
            version: m.counterpartyVersion
        });
        expectedChannel.connection_hops[0] = "counterparty-conn-id";

        bytes memory encodedChannel = Channel.encode(expectedChannel);
        m.proofInit = abi.encodePacked(sha256(encodedChannel));
        m.proofHeight.revision_height = proofHeight;
    }

    function channelOpenConfirm(string memory portId, string memory channelId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgChannelOpenConfirm memory m)
    {
        m.portId = portId;
        m.channelId = channelId;

        Channel.Data memory expectedChannel = Channel.Data({
            state: ChannelEnums.State.STATE_OPEN,
            ordering: ChannelEnums.Order.ORDER_NONE_UNSPECIFIED,
            counterparty: ChannelCounterparty.Data({port_id: portId, channel_id: channelId}),
            connection_hops: new string[](1),
            version: "counterparty-version"
        });
        expectedChannel.connection_hops[0] = "counterparty-conn-id";

        bytes memory encodedChannel = Channel.encode(expectedChannel);
        m.proofAck = abi.encodePacked(sha256(encodedChannel));
        m.proofHeight.revision_height = proofHeight;
    }

    function packetRecv(string memory portId, string memory channelId, uint64 proofHeight)
        internal
        view
        returns (IBCMsgs.MsgPacketRecv memory m)
    {
        m.packet.destination_port = portId;
        m.packet.destination_channel = channelId;
        m.packet.source_port = "counterparty-port-id";
        m.packet.source_channel = "counterparty-channel-id";
        m.packet.data = hex"12345678";
        m.packet.sequence = 1;

        m.proofHeight.revision_height = proofHeight;
        bytes32 commitmentBytes = sha256(
            abi.encodePacked(
                m.packet.timeout_timestamp,
                m.packet.timeout_height.revision_number,
                m.packet.timeout_height.revision_height,
                sha256(m.packet.data)
            )
        );
        m.proof = abi.encodePacked(sha256(abi.encodePacked(commitmentBytes)));
    }
}

function wrapAnyMockHeader(MockHeader.Data memory header) pure returns (bytes memory) {
    Any.Data memory anyHeader;
    anyHeader.type_url = "/ibc.lightclients.mock.v1.Header";
    anyHeader.value = MockHeader.encode(header);
    return Any.encode(anyHeader);
}

function wrapAnyMockClientState(MockClientState.Data memory clientState) pure returns (bytes memory) {
    Any.Data memory anyClientState;
    anyClientState.type_url = "/ibc.lightclients.mock.v1.ClientState";
    anyClientState.value = MockClientState.encode(clientState);
    return Any.encode(anyClientState);
}

function wrapAnyMockConsensusState(MockConsensusState.Data memory consensusState) pure returns (bytes memory) {
    Any.Data memory anyConsensusState;
    anyConsensusState.type_url = "/ibc.lightclients.mock.v1.ConsensusState";
    anyConsensusState.value = MockConsensusState.encode(consensusState);
    return Any.encode(anyConsensusState);
}
