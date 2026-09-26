use chart_requester::platforms::bilibili::{Packet, chat_from_command, decode, packet, wbi_key};
use serde_json::json;
use std::io::Write;
#[test]
fn concatenated_and_compressed_packets() {
    let command = json!({"cmd":"LIVE_OPEN_PLATFORM_DM","data":{"open_id":"viewer","uname":"test","msg":"点歌 AA SPA","msg_id":"abc"}});
    let mut combined = packet(8, b"{\"code\":0}");
    combined.extend(packet(5, command.to_string().as_bytes()));
    assert_eq!(decode(&combined).unwrap().len(), 2);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&combined).unwrap();
    let mut compressed = packet(5, &z.finish().unwrap());
    compressed[6..8].copy_from_slice(&2u16.to_be_bytes());
    let parsed = decode(&compressed).unwrap();
    assert!(matches!(parsed[0], Packet::Auth(0)));
    let Packet::Command(v) = &parsed[1] else {
        panic!()
    };
    let (chat, id) = chat_from_command(v).unwrap();
    assert_eq!(chat.user, "open:viewer");
    assert_eq!(id, "abc");
    let mut buffer = Vec::new();
    {
        let mut b = brotli::CompressorWriter::new(&mut buffer, 4096, 5, 22);
        b.write_all(&combined).unwrap();
    }
    let mut compressed = packet(5, &buffer);
    compressed[6..8].copy_from_slice(&3u16.to_be_bytes());
    assert_eq!(decode(&compressed).unwrap().len(), 2);
}
#[test]
fn malformed_headers_never_loop_or_panic() {
    for len in 1..16 {
        assert!(decode(&vec![0; len]).is_err());
    }
    let mut p = packet(5, b"{}");
    p[..4].copy_from_slice(&0u32.to_be_bytes());
    assert!(decode(&p).is_err());
    p[..4].copy_from_slice(&1000u32.to_be_bytes());
    assert!(decode(&p).is_err());
    let mut p = packet(5, b"{}");
    p[4..6].copy_from_slice(&1u16.to_be_bytes());
    assert!(decode(&p).is_err());
}
#[test]
fn decompression_bomb_bounded() {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&vec![0; 4 * 1024 * 1024 + 1]).unwrap();
    let mut p = packet(5, &z.finish().unwrap());
    p[6..8].copy_from_slice(&2u16.to_be_bytes());
    assert!(decode(&p).is_err());
}
#[test]
fn sender_identity_does_not_use_display_name() {
    let v =
        json!({"cmd":"DANMU_MSG:4:0:2:2:2:0","info":[[0,0,0,0,1000,8],"1",[123456,"same name"]]});
    assert_eq!(chat_from_command(&v).unwrap().0.user, "uid:123456");
    let mut v = v;
    v["info"][2][0] = json!(0);
    assert!(chat_from_command(&v).is_none());
    let v = json!({"cmd":"LIVE_OPEN_PLATFORM_DM","data":{"uname":"same name","msg":"1"}});
    assert!(chat_from_command(&v).is_none());
}
#[test]
fn heartbeat_echo_and_auth_failure() {
    let mut p = packet(3, &123u32.to_be_bytes());
    p.extend(b"{}");
    assert!(matches!(decode(&p).unwrap()[0], Packet::Heartbeat));
    assert!(matches!(
        decode(&packet(8, b"{\"code\":-101}")).unwrap()[0],
        Packet::Auth(-101)
    ));
}
#[test]
fn wbi_key_vector() {
    assert_eq!(
        wbi_key("7cd084941338484aae1ad9425b84077c4932c97a449d54cc5bfa42c9bc72f56b").unwrap(),
        "cc1d2124af3c70424d4b939a704a4748"
    );
    assert!(wbi_key("short").is_err());
}
