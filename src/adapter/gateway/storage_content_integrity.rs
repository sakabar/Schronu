pub(super) const DIGEST_ALGORITHM: &str = "fnv1a64";

pub(super) fn content_digest(bytes: &[u8]) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;

    let checksum = bytes.iter().fold(FNV_OFFSET_BASIS, |checksum, byte| {
        (checksum ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    });
    format!("{DIGEST_ALGORITHM}:{checksum:016x}")
}

pub(super) fn content_matches(bytes: &[u8], expected_length: u64, expected_digest: &str) -> bool {
    bytes.len() as u64 == expected_length && content_digest(bytes) == expected_digest
}
