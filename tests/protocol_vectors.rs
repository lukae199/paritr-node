use base64::Engine;
use paritr::{
    codec::ConsensusEncode,
    consensus::{self, Block},
};

#[test]
fn published_genesis_vector_matches_consensus_code() {
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../spec/test-vectors.json")).unwrap();
    let genesis = Block::genesis();

    assert_eq!(document["chain_id"], consensus::CHAIN_ID);
    assert_eq!(document["protocol_version"], consensus::PROTOCOL_VERSION);
    assert_eq!(document["header_version"], consensus::HEADER_VERSION);
    assert_eq!(
        document["genesis"]["message_utf8"],
        consensus::GENESIS_MESSAGE
    );
    assert_eq!(document["genesis"]["timestamp"], genesis.header.timestamp);
    assert_eq!(document["genesis"]["bits_decimal"], genesis.header.bits);
    assert_eq!(
        document["genesis"]["transactions_root"],
        genesis.header.transactions_root.to_string()
    );
    assert_eq!(
        document["codec"]["p2p_magic_ascii"],
        std::str::from_utf8(&paritr::p2p::WIRE_MAGIC).unwrap()
    );
    assert_eq!(document["genesis"]["block_id"], genesis.id().to_string());
    assert_eq!(
        document["genesis"]["consensus_base64"],
        base64::engine::general_purpose::STANDARD.encode(genesis.consensus_encode())
    );
    assert_eq!(
        document["genesis"]["state_root"],
        genesis.header.state_root.to_string()
    );
    assert_eq!(
        document["genesis"]["workshare_root"],
        genesis.header.workshare_root.to_string()
    );
    assert_eq!(document["limits"]["max_money"], consensus::MAX_MONEY);
    assert_eq!(
        document["timing"]["daa_window_blocks"],
        consensus::DAA_WINDOW
    );
}
