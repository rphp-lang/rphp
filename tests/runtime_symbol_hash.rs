use rphp::runtime::SymbolHasher;
use std::hash::Hasher;

// Captured before replacing the tail copy. These vectors protect existing
// symbol-table hashes, including non-UTF-8 bytes and all tail lengths.
#[test]
fn symbol_hash_preserves_existing_words_at_unaligned_starts() {
    let vectors = [
        (0, 0x0000000000000000),
        (1, 0x0c426097e786c90f),
        (2, 0xe1e032a82354550f),
        (3, 0xbcec6b98f55d550f),
        (4, 0x034e116f7b5d550f),
        (5, 0x6b9e57727b5d550f),
        (6, 0x70e8d7727b5d550f),
        (7, 0xbfe5d7727b5d550f),
        (8, 0x39e5d7727b5d550f),
        (9, 0xd407731f73997a6c),
        (10, 0xcdfbd746b526ae6c),
        (11, 0xe427b51c9d87ae6c),
        (12, 0xe003d8887787ae6c),
        (13, 0x22e4907d7787ae6c),
        (14, 0x8aa0287d7787ae6c),
        (15, 0xebe5287d7787ae6c),
        (16, 0x5de5287d7787ae6c),
        (17, 0x916138ba3063f848),
        (23, 0xb4737e2d166edc48),
        (24, 0xbe737e2d166edc48),
        (25, 0xcec8cc1e1ba327ac),
        (31, 0x89a94666eaf06bac),
        (32, 0x97a94666eaf06bac),
        (33, 0x83c3e4506acfc0b5),
        (255, 0x99a5f4a0853e23d4),
    ];
    for (len, expected) in vectors {
        for offset in 0..8 {
            let mut storage = vec![0xa5; offset + len + 8];
            for (index, byte) in storage[offset..offset + len].iter_mut().enumerate() {
                *byte = (index * 73 + 19) as u8;
            }
            let mut hasher = SymbolHasher::default();
            hasher.write(&storage[offset..offset + len]);
            assert_eq!(hasher.finish(), expected, "length {len}, offset {offset}");
        }
    }
}

#[test]
fn symbol_hash_preserves_incremental_and_integer_mixing() {
    let mut hasher = SymbolHasher::default();
    hasher.write(b"abc");
    hasher.write(b"\x00\xff\x01\x02\x03\x04\x05");
    hasher.write_u8(255);
    hasher.write_usize(0x123456);
    assert_eq!(hasher.finish(), 0xfb128f978d991ebf);
}
