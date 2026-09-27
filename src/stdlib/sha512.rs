//! SHA-512 for phar signatures and `hash()`. The digest core comes from the
//! `sha2` crate that the password-hashing dependencies already pull in; a
//! hand-written scalar compression spent ~40 instructions per byte and was the
//! single largest item of a PHPStan bootstrap (a 29 MB phar is hashed once).

use sha2::Digest;

pub(crate) fn sha512_digest(input: &[u8]) -> [u8; 64] {
    sha2::Sha512::digest(input).into()
}

#[cfg(test)]
mod tests {
    use super::sha512_digest;

    fn hex(digest: &[u8]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn matches_the_fips_test_vectors() {
        assert_eq!(
            hex(&sha512_digest(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512_digest(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        let long: Vec<u8> = (0..5).flat_map(|_| 0..=255u8).collect();
        assert_eq!(
            hex(&sha512_digest(&long)),
            "c93f55ccf2fa8c82699ff9b58afe3591242b135d908a6d865e17e38adb41c21d1d5359e51273036373d54d20b5659cc87e6e7b381ff027d33f971416cc590f90"
        );
    }
}
