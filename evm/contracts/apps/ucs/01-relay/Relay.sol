pragma solidity ^0.8.23;

import "@openzeppelin/token/ERC20/ERC20.sol";
import "@openzeppelin/token/ERC20/IERC20.sol";
import "@openzeppelin/token/ERC20/utils/SafeERC20.sol";
import "solady/utils/LibString.sol";
import "solidity-stringutils/strings.sol";
import "../../../core/25-handler/IBCHandler.sol";
import "../../Base.sol";
import "./IERC20Denom.sol";
import "./ERC20Denom.sol";
import "../../../lib/Hex.sol";

// NOTE: uint128 limitation from cosmwasm_std Coin type for transfers.
struct LocalToken {
    address denom;
    uint128 amount;
}

struct Token {
    string denom;
    uint256 amount;
}

struct RelayPacket {
    bytes sender;
    bytes receiver;
    Token[] tokens;
}

interface IRelay is IIBCModule {
    function getDenomAddress(
        string memory sourcePort,
        ChannelId sourceChannel,
        string memory denom
    ) external view returns (address);

    function getOutstanding(
        string memory sourcePort,
        ChannelId sourceChannel,
        address token
    ) external view returns (uint256);

    function getCounterpartyEndpoint(
        string memory sourcePort,
        ChannelId sourceChannel
    ) external view returns (IBCChannelTypes.Counterparty memory);

    function send(
        string calldata sourcePort,
        ChannelId sourceChannel,
        bytes calldata receiver,
        LocalToken[] calldata tokens,
        uint64 counterpartyTimeoutRevisionNumber,
        uint64 counterpartyTimeoutRevisionHeight
    ) external;
}

library RelayLib {
    using LibString for *;

    error ErrInvalidBytesAddress();
    error ErrUnauthorized();
    error ErrInvalidAcknowledgement();
    error ErrInvalidProtocolVersion();
    error ErrInvalidProtocolOrdering();
    error ErrInvalidCounterpartyProtocolVersion();
    error ErrUnstoppable();

    IbcCoreChannelV1GlobalEnums.Order public constant ORDER =
        IbcCoreChannelV1GlobalEnums.Order.ORDER_UNORDERED;
    string public constant VERSION = "ucs01-0";
    bytes1 public constant ACK_SUCCESS = 0x01;
    bytes1 public constant ACK_FAILURE = 0x00;
    uint256 public constant ACK_LENGTH = 1;

    event DenomCreated(string denom, address token);
    event Received(
        string sender,
        address receiver,
        string denom,
        address token,
        uint256 amount
    );
    event Sent(
        address sender,
        string receiver,
        string denom,
        address token,
        uint256 amount
    );
    event Refunded(
        address sender,
        string receiver,
        string denom,
        address token,
        uint256 amount
    );

    function isValidVersion(string memory version)
        internal
        pure
        returns (bool)
    {
        return version.eq(VERSION);
    }

    function isFromChannel(
        string memory portId,
        ChannelId channelId,
        string memory denom
    ) internal pure returns (bool) {
        return bytes(denom).length > 0
            && denom.startsWith(makeDenomPrefix(portId, channelId));
    }

    function makeDenomPrefix(
        string memory portId,
        ChannelId channelId
    ) internal pure returns (string memory) {
        return string(abi.encodePacked(portId, "/", channelId.toString(), "/"));
    }

    function makeForeignDenom(
        string memory portId,
        ChannelId channelId,
        string memory denom
    ) internal pure returns (string memory) {
        return
            string(abi.encodePacked(makeDenomPrefix(portId, channelId), denom));
    }

    function bytesToAddress(bytes memory b) internal pure returns (address) {
        if (b.length != 20) {
            revert ErrInvalidBytesAddress();
        }
        return address(uint160(bytes20(b)));
    }
}

library RelayPacketLib {
    function encode(RelayPacket memory packet)
        internal
        pure
        returns (bytes memory)
    {
        return abi.encode(packet.sender, packet.receiver, packet.tokens);
    }

    function decode(bytes memory packet)
        internal
        pure
        returns (RelayPacket memory)
    {
        (bytes memory sender, bytes memory receiver, Token[] memory tokens) =
            abi.decode(packet, (bytes, bytes, Token[]));
        return RelayPacket({sender: sender, receiver: receiver, tokens: tokens});
    }
}

contract UCS01Relay is IBCAppBase, IRelay {
    using RelayPacketLib for RelayPacket;
    using LibString for *;
    using strings for *;
    using {parseChannelIdCalldata} for string;

    IBCHandler private immutable ibcHandler;

    // A mapping from remote denom to local ERC20 wrapper.
    // port -> channel -> denom -> address
    mapping(string => mapping(ChannelId => mapping(string => address))) private
        denomToAddress;
    // A mapping from a local ERC20 wrapper to the remote denom.
    // Required to determine whether an ERC20 token is originating from a remote chain.
    // port -> channel -> denom -> address
    mapping(string => mapping(ChannelId => mapping(address => string))) private
        addressToDenom;
    // A mapping from local port/channel to it's counterparty.
    // This is required to remap denoms.
    mapping(string => mapping(ChannelId => IBCChannelTypes.Counterparty))
        private counterpartyEndpoints;
    mapping(string => mapping(ChannelId => mapping(address => uint256))) private
        outstanding;

    constructor(IBCHandler _ibcHandler) {
        ibcHandler = _ibcHandler;
    }

    function ibcAddress() public view virtual override returns (address) {
        return address(ibcHandler);
    }

    // Return the ERC20 wrapper for the given remote-native denom.
    function getDenomAddress(
        string memory sourcePort,
        ChannelId sourceChannel,
        string memory denom
    ) external view override returns (address) {
        return denomToAddress[sourcePort][sourceChannel][denom];
    }

    // Return the amount of tokens submitted through the given port/channel.
    function getOutstanding(
        string memory sourcePort,
        ChannelId sourceChannel,
        address token
    ) external view override returns (uint256) {
        return outstanding[sourcePort][sourceChannel][token];
    }

    // Return a channel counterparty endpoint.
    // A counterparty will exist only if a channel has been previously opened.
    function getCounterpartyEndpoint(
        string memory sourcePort,
        ChannelId sourceChannel
    ) external view override returns (IBCChannelTypes.Counterparty memory) {
        return counterpartyEndpoints[sourcePort][sourceChannel];
    }

    // Increase the oustanding amount on the given port/channel.
    // Happens when we send the token.
    function increaseOutstanding(
        string memory sourcePort,
        ChannelId sourceChannel,
        address token,
        uint256 amount
    ) internal {
        outstanding[sourcePort][sourceChannel][token] += amount;
    }

    // Decrease the outstanding amount on the given port/channel.
    // Happens either when receiving previously sent tokens or when refunding.
    function decreaseOutstanding(
        string memory sourcePort,
        ChannelId sourceChannel,
        address token,
        uint256 amount
    ) internal {
        outstanding[sourcePort][sourceChannel][token] -= amount;
    }

    // Internal function
    // Send the given token over the specified channel.
    // If token is native, we increase the oustanding amount and escrow it. Otherwise, we burn the amount.
    // The operation is symmetric with the counterparty, if we burn locally, the remote relay will unescrow. If we escrow locally, the remote relay will mint.
    function sendToken(
        string calldata sourcePort,
        ChannelId sourceChannel,
        string memory counterpartyPortId,
        ChannelId counterpartyChannelId,
        LocalToken calldata localToken
    ) internal returns (string memory addressDenom) {
        // Ensure the user properly fund us.
        SafeERC20.safeTransferFrom(
            IERC20(localToken.denom),
            msg.sender,
            address(this),
            localToken.amount
        );
        // If the token is originating from the counterparty channel, we must have saved it's denom.
        addressDenom =
            addressToDenom[sourcePort][sourceChannel][localToken.denom];
        if (bytes(addressDenom).length != 0) {
            // Token originating from the remote chain, burn the amount.
            IERC20Denom(localToken.denom).burn(address(this), localToken.amount);
        } else {
            // Token originating from the local chain, increase outstanding and escrow the amount.
            increaseOutstanding(
                sourcePort, sourceChannel, localToken.denom, localToken.amount
            );
            addressDenom = localToken.denom.toHexString();
        }
    }

    function send(
        string calldata sourcePort,
        ChannelId sourceChannel,
        bytes calldata receiver,
        LocalToken[] calldata tokens,
        uint64 counterpartyTimeoutRevisionNumber,
        uint64 counterpartyTimeoutRevisionHeight
    ) external override {
        IBCChannelTypes.Counterparty memory counterparty =
            counterpartyEndpoints[sourcePort][sourceChannel];
        Token[] memory normalizedTokens = new Token[](tokens.length);
        for (uint256 i = 0; i < tokens.length; i++) {
            LocalToken calldata localToken = tokens[i];
            string memory addressDenom = sendToken(
                sourcePort,
                sourceChannel,
                counterparty.portId,
                counterparty.channelId,
                localToken
            );
            normalizedTokens[i].denom = addressDenom;
            normalizedTokens[i].amount = uint256(localToken.amount);
            emit RelayLib.Sent(
                msg.sender,
                receiver.toHexString(),
                addressDenom,
                localToken.denom,
                uint256(localToken.amount)
            );
        }
        string memory sender = msg.sender.toHexString();
        RelayPacket memory packet = RelayPacket({
            sender: abi.encodePacked(msg.sender),
            receiver: receiver,
            tokens: normalizedTokens
        });
        IbcCoreClientV1Height.Data memory timeoutHeight = IbcCoreClientV1Height
            .Data({
            revision_number: counterpartyTimeoutRevisionNumber,
            revision_height: counterpartyTimeoutRevisionHeight
        });
        ibcHandler.sendPacket(
            sourcePort,
            sourceChannel,
            timeoutHeight,
            // TODO: do we want to allow both height and timestamp timeouts?
            0,
            packet.encode()
        );
    }

    function refundTokens(
        string memory portId,
        ChannelId channelId,
        RelayPacket memory packet
    ) internal {
        string memory receiver = packet.receiver.toHexString();
        // We're going to refund, the receiver will be the sender.
        address userToRefund = RelayLib.bytesToAddress(packet.sender);
        for (uint256 i = 0; i < packet.tokens.length; i++) {
            Token memory token = packet.tokens[i];
            address denomAddress =
                denomToAddress[portId][channelId][token.denom];
            if (denomAddress != address(0)) {
                // The token was originating from the remote chain, we burnt it.
                // Refund means minting in this case.
                IERC20Denom(denomAddress).mint(userToRefund, token.amount);
            } else {
                // The token was originating from the local chain, we escrowed
                // it. Refund means unescrowing.

                // It's an ERC20 string 0x prefixed hex address
                denomAddress = Hex.hexToAddress(token.denom);
                decreaseOutstanding(
                    portId, channelId, denomAddress, token.amount
                );
                IERC20(denomAddress).transfer(userToRefund, token.amount);
            }
            emit RelayLib.Refunded(
                userToRefund, receiver, token.denom, denomAddress, token.amount
            );
        }
    }

    function onRecvPacketProcessing(
        IbcCoreChannelV1Packet.Data calldata ibcPacket,
        address relayer
    ) public {
        if (msg.sender != address(this)) {
            revert RelayLib.ErrUnauthorized();
        }
        RelayPacket memory packet = RelayPacketLib.decode(ibcPacket.data);

        ChannelId destinationChannelId =
            ibcPacket.destination_channel.parseChannelIdCalldata();
        ChannelId sourceChannelId =
            ibcPacket.source_channel.parseChannelIdCalldata();

        string memory prefix = RelayLib.makeDenomPrefix(
            ibcPacket.destination_port, destinationChannelId
        );
        for (uint256 i = 0; i < packet.tokens.length; i++) {
            Token memory token = packet.tokens[i];
            strings.slice memory denomSlice = token.denom.toSlice();
            // This will trim the denom IFF it is prefixed
            strings.slice memory trimedDenom =
                denomSlice.beyond(prefix.toSlice());
            address receiver = RelayLib.bytesToAddress(packet.receiver);
            address denomAddress;
            string memory denom;
            if (!denomSlice.equals(token.denom.toSlice())) {
                // In this branch the token was originating from
                // this chain as it was prefixed by the local channel/port.
                // We need to unescrow the amount.
                denom = trimedDenom.toString();
                // It's an ERC20 string 0x prefixed hex address
                denomAddress = Hex.hexToAddress(denom);
                // The token must be outstanding.
                decreaseOutstanding(
                    ibcPacket.destination_port,
                    destinationChannelId,
                    denomAddress,
                    token.amount
                );
                IERC20(denomAddress).transfer(receiver, token.amount);
            } else {
                // In this branch the token was originating from the
                // counterparty chain. We need to mint the amount.
                denom = RelayLib.makeForeignDenom(
                    ibcPacket.source_port, sourceChannelId, token.denom
                );
                denomAddress = denomToAddress[ibcPacket.destination_port][destinationChannelId][denom];
                if (denomAddress == address(0)) {
                    denomAddress = address(new ERC20Denom(denom));
                    denomToAddress[ibcPacket.destination_port][destinationChannelId][denom]
                    = denomAddress;
                    addressToDenom[ibcPacket.destination_port][destinationChannelId][denomAddress]
                    = denom;
                    emit RelayLib.DenomCreated(denom, denomAddress);
                }
                IERC20Denom(denomAddress).mint(receiver, token.amount);
            }
            emit RelayLib.Received(
                packet.sender.toHexString(),
                receiver,
                denom,
                denomAddress,
                token.amount
            );
        }
    }

    function onRecvPacket(
        IbcCoreChannelV1Packet.Data calldata ibcPacket,
        address relayer
    )
        external
        override(IBCAppBase, IIBCModule)
        onlyIBC
        returns (bytes memory)
    {
        // TODO: maybe consider threading _res in the failure ack
        (bool success, bytes memory _res) = address(this).call(
            abi.encodeWithSelector(
                this.onRecvPacketProcessing.selector, ibcPacket, relayer
            )
        );
        // We make sure not to revert to allow the failure ack to be sent back,
        // resulting in a refund.
        if (success) {
            return abi.encodePacked(RelayLib.ACK_SUCCESS);
        } else {
            return abi.encodePacked(RelayLib.ACK_FAILURE);
        }
    }

    function onAcknowledgementPacket(
        IbcCoreChannelV1Packet.Data calldata ibcPacket,
        bytes calldata acknowledgement,
        address _relayer
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        if (
            acknowledgement.length != RelayLib.ACK_LENGTH
                || (
                    acknowledgement[0] != RelayLib.ACK_FAILURE
                        && acknowledgement[0] != RelayLib.ACK_SUCCESS
                )
        ) {
            revert RelayLib.ErrInvalidAcknowledgement();
        }
        RelayPacket memory packet = RelayPacketLib.decode(ibcPacket.data);
        // Counterparty failed to execute the transfer, we refund.
        if (acknowledgement[0] == RelayLib.ACK_FAILURE) {
            refundTokens(
                ibcPacket.source_port,
                ibcPacket.source_channel.parseChannelIdCalldata(),
                packet
            );
        }
    }

    function onTimeoutPacket(
        IbcCoreChannelV1Packet.Data calldata ibcPacket,
        address _relayer
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        refundTokens(
            ibcPacket.source_port,
            ibcPacket.source_channel.parseChannelIdCalldata(),
            RelayPacketLib.decode(ibcPacket.data)
        );
    }

    function onChanOpenInit(
        IbcCoreChannelV1GlobalEnums.Order order,
        string[] calldata _connectionHops,
        string calldata portId,
        ChannelId channelId,
        IBCChannelTypes.Counterparty calldata counterpartyEndpoint,
        string calldata version
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        if (!RelayLib.isValidVersion(version)) {
            revert RelayLib.ErrInvalidProtocolVersion();
        }
        if (order != RelayLib.ORDER) {
            revert RelayLib.ErrInvalidProtocolOrdering();
        }
        counterpartyEndpoints[portId][channelId] = counterpartyEndpoint;
    }

    function onChanOpenTry(
        IbcCoreChannelV1GlobalEnums.Order order,
        string[] calldata _connectionHops,
        string calldata portId,
        ChannelId channelId,
        IBCChannelTypes.Counterparty calldata counterpartyEndpoint,
        string calldata version,
        string calldata counterpartyVersion
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        if (!RelayLib.isValidVersion(version)) {
            revert RelayLib.ErrInvalidProtocolVersion();
        }
        if (order != RelayLib.ORDER) {
            revert RelayLib.ErrInvalidProtocolOrdering();
        }
        if (!RelayLib.isValidVersion(counterpartyVersion)) {
            revert RelayLib.ErrInvalidCounterpartyProtocolVersion();
        }
        counterpartyEndpoints[portId][channelId] = counterpartyEndpoint;
    }

    function onChanOpenAck(
        string calldata portId,
        ChannelId channelId,
        ChannelId counterpartyChannelId,
        string calldata counterpartyVersion
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        if (!RelayLib.isValidVersion(counterpartyVersion)) {
            revert RelayLib.ErrInvalidCounterpartyProtocolVersion();
        }
        // Counterparty channel was empty.
        counterpartyEndpoints[portId][channelId].channelId =
            counterpartyChannelId;
    }

    function onChanOpenConfirm(
        string calldata _portId,
        ChannelId _channelId
    ) external override(IBCAppBase, IIBCModule) onlyIBC {}

    function onChanCloseInit(
        string calldata _portId,
        ChannelId _channelId
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        revert RelayLib.ErrUnstoppable();
    }

    function onChanCloseConfirm(
        string calldata _portId,
        ChannelId _channelId
    ) external override(IBCAppBase, IIBCModule) onlyIBC {
        revert RelayLib.ErrUnstoppable();
    }
}
