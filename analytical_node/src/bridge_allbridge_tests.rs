use super::*;

fn word(value: u64) -> [u8; 32] {
    let mut word = [0; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn hash(network: Network, byte: &str) -> String {
    let prefix = if network == Network::Tron { "" } else { "0x" };
    format!("{prefix}{}", byte.repeat(64))
}

fn log(network: Network, topic: &str, data: &[[u8; 32]], at: u64) -> Log {
    Log {
        tx_hash: hash(network, "a"),
        block_hash: hash(network, "b"),
        block_number: 7,
        timestamp_ms: at,
        log_index: 2,
        address: protocol::emitter(network).unwrap().into(),
        topics: vec![topic.into()],
        data: data.iter().map(|word| hex(word)[2..].to_owned()).collect(),
    }
}

pub(in crate::bridge) fn fixture(source: Network) -> (Log, Log) {
    let destination = protocol::peer(source).unwrap();
    let source_id = protocol::network_id(source).unwrap();
    let destination_id = protocol::network_id(destination).unwrap();
    let (token, _, _) = protocol::destination_usdt(destination).unwrap();
    let send = log(
        source,
        protocol::SEND,
        &[
            word(100_000),
            word(2025),
            word(destination_id.into()),
            token,
            word(9),
            word(1),
        ],
        1_700_000_000_000,
    );
    let leg = protocol::decode(source, &send).unwrap();
    let id = message(source, source_id, destination_id, &leg, &token).unwrap();
    let receive = log(
        destination,
        protocol::RECEIVE,
        &[word(99_000_000), leg.recipient, leg.nonce, word(1), id],
        send.timestamp_ms + 1_000,
    );
    (send, receive)
}

fn match_logs(source: Network, send: &Log, receive: &Log) -> Option<Value> {
    let destination = protocol::peer(source)?;
    let a = protocol::decode(source, send)?;
    let b = protocol::decode(destination, receive)?;
    paired(
        (source, send, &a, &Value::Null),
        (destination, receive, &b, &Value::Null),
    )
}

#[test]
fn hashes_exact_message_for_both_directions_and_rejects_changed_witnesses() {
    for network in [Network::Bsc, Network::Tron] {
        let (send, receive) = fixture(network);
        let trace = match_logs(network, &send, &receive).unwrap();
        assert_eq!(trace["send"]["amount_raw"], "100000");
        assert_eq!(trace["send"]["amount_unit"], "protocol_vusd");
        assert_eq!(trace["relay"]["amount_raw"], "99000000");
        assert_eq!(trace["asset_scope"], "destination");
        assert_eq!(trace["settlement_verified"], false);
        for (offset, changed) in [
            (63, "f"),
            (127, "f"),
            (191, "f"),
            (255, "f"),
            (319, "f"),
            (383, "2"),
        ] {
            let mut altered = send.clone();
            altered.data.replace_range(offset..offset + 1, changed);
            assert!(
                match_logs(network, &altered, &receive).is_none(),
                "{offset}"
            );
        }
        let mut changed_message = receive.clone();
        changed_message.data.replace_range(300..301, "f");
        if changed_message.data == receive.data {
            changed_message.data.replace_range(300..301, "e");
        }
        assert!(match_logs(network, &send, &changed_message).is_none());
        let mut late = receive.clone();
        late.timestamp_ms = send.timestamp_ms + WINDOW_MS + 1;
        assert!(match_logs(network, &send, &late).is_none());
    }
}

#[test]
fn recomputes_message_from_confirmed_tron_receive_call() {
    // Parameters from the public receiveTokens call fbb641...7ec6c and
    // its TokensReceived event at log 2. The fixture only checks hashing.
    let source = Network::Bsc;
    let (token, _, _) = protocol::destination_usdt(Network::Tron).unwrap();
    let mut input = log(
        source,
        protocol::SEND,
        &[word(0x30929b2), word(0), word(3), token, word(0), word(1)],
        1,
    );
    input.data.replace_range(
        64..128,
        "00000000000000000000000077da60e9c23ac730d9a40a6e35f64cddcb1a86ef",
    );
    input.data.replace_range(
        256..320,
        "d395712d73e0e55a956f4431a904729f438e9d5a7bf73ce5646cc7efb05c905e",
    );
    let leg = protocol::decode(source, &input).unwrap();
    assert_eq!(
        hex(&message(source, 2, 3, &leg, &token).unwrap()),
        "0x0203083ccb1c2d01fefd17d2210c70e075b08ce1c61152f3fdf11f075515c7fa"
    );
}

#[test]
#[ignore = "requires ALLBRIDGE_RPC_FIXTURE containing the two public canonical logs"]
fn public_allbridge_messages_pair_from_native_receipts() -> anyhow::Result<()> {
    let path = std::env::var_os("ALLBRIDGE_RPC_FIXTURE").context("set ALLBRIDGE_RPC_FIXTURE")?;
    let (source, send, receive): (String, Log, Log) =
        serde_json::from_slice(&std::fs::read(path)?)?;
    let network = Network::parse(&source)?;
    let result =
        match_logs(network, &send, &receive).context("real Allbridge message did not match")?;
    assert_eq!(result["protocol"], "allbridge_core");
    assert_eq!(result["asset"], "USDT");
    Ok(())
}
