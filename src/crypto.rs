use std::{fmt, str::FromStr};

use ripemd::Ripemd160;
use secp256k1::{ecdsa::Signature, Message, PublicKey, Secp256k1};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::codec::{ConsensusDecode, ConsensusEncode, Reader, Writer};

pub const ADDRESS_VERSION: u8 = 55;
pub const ADDRESS_ENCODED_BYTES: usize = 25;

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub const ZERO: Self = Self([0; 32]);

    pub fn from_slice(bytes: &[u8]) -> Result<Self, CryptoError> {
        let value: [u8; 32] = bytes.try_into().map_err(|_| CryptoError::InvalidHash)?;
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl FromStr for Hash32 {
    type Err = CryptoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64 || value.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
            return Err(CryptoError::InvalidHash);
        }
        let bytes = hex::decode(value).map_err(|_| CryptoError::InvalidHash)?;
        Self::from_slice(&bytes)
    }
}

impl Serialize for Hash32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hash32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

impl ConsensusEncode for Hash32 {
    fn encode_to(&self, writer: &mut Writer) {
        writer.fixed(&self.0);
    }
}

impl ConsensusDecode for Hash32 {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, crate::codec::CodecError> {
        Ok(Self(reader.fixed()?))
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Address([u8; ADDRESS_ENCODED_BYTES]);

impl Address {
    pub fn from_public_key_hash(key_hash: [u8; 20]) -> Self {
        let mut payload = [0_u8; ADDRESS_ENCODED_BYTES];
        payload[0] = ADDRESS_VERSION;
        payload[1..21].copy_from_slice(&key_hash);
        let checksum = sha256d(&payload[..21]);
        payload[21..].copy_from_slice(&checksum.0[..4]);
        Self(payload)
    }

    pub fn from_public_key(public_key: &PublicKey) -> Self {
        // Protocol 8 addresses hash the uncompressed SEC1 key. Keeping that rule
        // allows existing private keys and P... addresses to survive the fork.
        let key_hash = hash160(&public_key.serialize_uncompressed());
        Self::from_public_key_hash(key_hash)
    }

    pub fn from_bytes(bytes: [u8; ADDRESS_ENCODED_BYTES]) -> Result<Self, CryptoError> {
        if bytes[0] != ADDRESS_VERSION {
            return Err(CryptoError::AddressVersion(bytes[0]));
        }
        if sha256d(&bytes[..21]).0[..4] != bytes[21..] {
            return Err(CryptoError::AddressChecksum);
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; ADDRESS_ENCODED_BYTES] {
        &self.0
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&bs58::encode(self.0).into_string())
    }
}

impl FromStr for Address {
    type Err = CryptoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let decoded = bs58::decode(value)
            .into_vec()
            .map_err(|_| CryptoError::InvalidAddress)?;
        let bytes: [u8; ADDRESS_ENCODED_BYTES] = decoded
            .try_into()
            .map_err(|_| CryptoError::InvalidAddress)?;
        let address = Self::from_bytes(bytes)?;
        if address.to_string() != value {
            return Err(CryptoError::NonCanonicalAddress);
        }
        Ok(address)
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

impl ConsensusEncode for Address {
    fn encode_to(&self, writer: &mut Writer) {
        writer.fixed(&self.0);
    }
}

impl ConsensusDecode for Address {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, crate::codec::CodecError> {
        Self::from_bytes(reader.fixed()?)
            .map_err(|_| crate::codec::CodecError::NonCanonical("invalid address"))
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum CryptoError {
    #[error("invalid 32-byte hash")]
    InvalidHash,
    #[error("invalid address encoding")]
    InvalidAddress,
    #[error("address uses version {0}, expected 55")]
    AddressVersion(u8),
    #[error("address checksum mismatch")]
    AddressChecksum,
    #[error("address is not canonically encoded")]
    NonCanonicalAddress,
    #[error("invalid secp256k1 public key")]
    InvalidPublicKey,
    #[error("invalid compact ECDSA signature")]
    InvalidSignature,
    #[error("ECDSA signature has a high S value")]
    HighSignatureS,
    #[error("public key does not derive the sender address")]
    SenderMismatch,
}

pub fn sha256(data: &[u8]) -> Hash32 {
    Hash32(Sha256::digest(data).into())
}

pub fn sha256d(data: &[u8]) -> Hash32 {
    let first = Sha256::digest(data);
    Hash32(Sha256::digest(first).into())
}

pub fn domain_hash(domain: &[u8], data: &[u8]) -> Hash32 {
    let mut hasher = Sha256::new();
    let domain_len = u32::try_from(domain.len()).expect("domain separator length fits in u32");
    hasher.update(domain_len.to_le_bytes());
    hasher.update(domain);
    hasher.update((data.len() as u64).to_le_bytes());
    hasher.update(data);
    let first = hasher.finalize();
    Hash32(Sha256::digest(first).into())
}

pub fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    Ripemd160::digest(sha).into()
}

pub fn parse_and_validate_signature(
    sender: Address,
    public_key: &[u8],
    signature: &[u8; 64],
    digest: Hash32,
) -> Result<(), CryptoError> {
    let public_key =
        PublicKey::from_slice(public_key).map_err(|_| CryptoError::InvalidPublicKey)?;
    if Address::from_public_key(&public_key) != sender {
        return Err(CryptoError::SenderMismatch);
    }
    let signature =
        Signature::from_compact(signature).map_err(|_| CryptoError::InvalidSignature)?;
    let mut normalized = signature;
    normalized.normalize_s();
    if normalized.serialize_compact() != signature.serialize_compact() {
        return Err(CryptoError::HighSignatureS);
    }
    let message = Message::from_digest(digest.0);
    Secp256k1::verification_only()
        .verify_ecdsa(&message, &signature, &public_key)
        .map_err(|_| CryptoError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::SecretKey;

    #[test]
    fn address_round_trip_preserves_protocol_8_derivation() {
        let secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
        let public = PublicKey::from_secret_key(&Secp256k1::new(), &secret);
        let address = Address::from_public_key(&public);
        assert!(address.to_string().starts_with('P'));
        assert_eq!(address.to_string().parse::<Address>().unwrap(), address);
    }

    #[test]
    fn hashes_are_domain_separated() {
        assert_ne!(domain_hash(b"a", b"bc"), domain_hash(b"ab", b"c"));
        assert_ne!(sha256(b"x"), sha256d(b"x"));
    }

    #[test]
    fn compact_signature_is_bound_to_sender_and_digest() {
        let secret = SecretKey::from_slice(&[9_u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let public = PublicKey::from_secret_key(&secp, &secret);
        let digest = domain_hash(b"test", b"signed payload");
        let signature = secp
            .sign_ecdsa(&Message::from_digest(digest.0), &secret)
            .serialize_compact();
        parse_and_validate_signature(
            Address::from_public_key(&public),
            &public.serialize(),
            &signature,
            digest,
        )
        .unwrap();
        assert!(parse_and_validate_signature(
            Address::from_public_key_hash([1; 20]),
            &public.serialize(),
            &signature,
            digest,
        )
        .is_err());
        assert!(parse_and_validate_signature(
            Address::from_public_key(&public),
            &public.serialize(),
            &signature,
            domain_hash(b"test", b"other payload"),
        )
        .is_err());
    }
}
