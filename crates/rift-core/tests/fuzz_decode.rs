use bytes::Bytes;
use rand::{thread_rng, Rng};
use rift_core::{decode_msg, PhysicalPacket, RIFT_MAGIC, RIFT_VERSION};

#[test]
fn fuzz_decode_message_never_panics() {
    let mut rng = thread_rng();
    for _ in 0..10_000 {
        let len: usize = rng.gen_range(0..2048);
        let mut data = vec![0u8; len];
        rng.fill(&mut data[..]);
        let _ = decode_msg(&data);
    }
}

#[test]
fn fuzz_decode_physical_packet_never_panics() {
    let mut rng = thread_rng();
    for _ in 0..10_000 {
        let len: usize = rng.gen_range(0..2048);
        let mut data = vec![0u8; len];
        rng.fill(&mut data[..]);
        let _ = PhysicalPacket::decode(Bytes::from(data));
    }
}

#[test]
fn random_mutation_of_valid_header_is_handled() {
    let mut rng = thread_rng();
    let mut packet = vec![0u8; 18];
    packet[0..2].copy_from_slice(&RIFT_MAGIC);
    packet[2..4].copy_from_slice(&RIFT_VERSION.to_be_bytes());

    for _ in 0..1_000 {
        let mut mutated = packet.clone();
        let flip_count = rng.gen_range(1..6);
        for _ in 0..flip_count {
            let idx = rng.gen_range(0..mutated.len());
            mutated[idx] ^= rng.gen::<u8>();
        }
        let _ = PhysicalPacket::decode(Bytes::from(mutated));
    }
}
