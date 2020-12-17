//! Utilities used in tx module.

// External uses.
use serde::{
    de::{value::SeqAccessDeserializer, Error, SeqAccess, Visitor},
    Deserialize, Deserializer,
};

/// Deserializes either a `String` or `Vec<u8>` into `Vec<u8>`.
/// The reason we cannot expect just a vector is backward compatibility: messages
/// used to be stored as strings.
pub fn deserialize_eth_message<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrVec;

    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a byte array or a string")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(v.as_bytes().to_vec())
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            Deserialize::deserialize(SeqAccessDeserializer::new(seq))
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

/// Serialize `H256` as `Vec<u8>`.
///
/// This workaround used for backward compatibility
/// with the old serialize/deserialize behaviour of the fields
/// whose type changed from `Vec<u8>` to `H256`.
pub mod h256_as_vec {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use zksync_basic_types::H256;

    pub fn serialize<S>(val: &H256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let val = val.as_bytes().to_vec();
        val.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<H256, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::deserialize(deserializer).map(|val| H256::from_slice(&val))
    }
}
