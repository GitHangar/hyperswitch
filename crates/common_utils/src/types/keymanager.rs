#![allow(missing_docs)]

use core::fmt;

use base64::Engine;
use error_stack::ResultExt;
use masking::{ExposeInterface, PeekInterface, Secret, Strategy, StrongSecret};
use rustc_hash::FxHashMap;
use serde::{
    de::{self, Unexpected, Visitor},
    ser, Deserialize, Deserializer, Serialize,
};

use crate::{
    consts::BASE64_ENGINE,
    crypto::Encryptable,
    encryption::Encryption,
    errors::{self, CustomResult},
    transformers::{ForeignFrom, ForeignTryFrom},
};

#[derive(Debug)]
pub struct KeyManagerState {
    pub url: String,
    pub client_idle_timeout: Option<u64>,
    #[cfg(feature = "keymanager_mtls")]
    pub ca: Secret<String>,
    #[cfg(feature = "keymanager_mtls")]
    pub cert: Secret<String>,
}
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
#[serde(tag = "data_identifier", content = "key_identifier")]
pub enum Identifier {
    User(String),
    Merchant(String),
}

pub const DEFAULT_KEY: &str = "DEFAULT";

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct EncryptionCreateRequest {
    #[serde(flatten)]
    pub identifier: Identifier,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
pub struct EncryptionTransferRequest {
    #[serde(flatten)]
    pub identifier: Identifier,
    pub key: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DataKeyCreateResponse {
    #[serde(flatten)]
    pub identifier: Identifier,
    pub key_version: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EncryptDataRequest {
    #[serde(flatten)]
    pub identifier: Identifier,
    pub data: DecryptedDataGroup,
}

impl<S> From<(Secret<Vec<u8>, S>, Identifier)> for EncryptDataRequest
where
    S: Strategy<Vec<u8>>,
{
    fn from((secret, identifier): (Secret<Vec<u8>, S>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        group.insert(
            DEFAULT_KEY.to_string(),
            DecryptedData(StrongSecret::new(secret.expose())),
        );
        Self {
            identifier,
            data: DecryptedDataGroup(group),
        }
    }
}

impl<S> From<(FxHashMap<String, Secret<Vec<u8>, S>>, Identifier)> for EncryptDataRequest
where
    S: Strategy<Vec<u8>>,
{
    fn from((map, identifier): (FxHashMap<String, Secret<Vec<u8>, S>>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        for (key, value) in map.iter() {
            group.insert(
                key.clone(),
                DecryptedData(StrongSecret::new(value.clone().expose())),
            );
        }
        Self {
            identifier,
            data: DecryptedDataGroup(group),
        }
    }
}

impl<S> From<(Secret<String, S>, Identifier)> for EncryptDataRequest
where
    S: Strategy<String>,
{
    fn from((secret, identifier): (Secret<String, S>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        group.insert(
            DEFAULT_KEY.to_string(),
            DecryptedData(StrongSecret::new(
                secret.clone().expose().as_bytes().to_vec(),
            )),
        );
        Self {
            data: DecryptedDataGroup(group),
            identifier: identifier.clone(),
        }
    }
}

impl<S> From<(Secret<serde_json::Value, S>, Identifier)> for EncryptDataRequest
where
    S: Strategy<serde_json::Value>,
{
    fn from((secret, identifier): (Secret<serde_json::Value, S>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        group.insert(
            DEFAULT_KEY.to_string(),
            DecryptedData(StrongSecret::new(
                secret.clone().expose().to_string().as_bytes().to_vec(),
            )),
        );
        Self {
            data: DecryptedDataGroup(group),
            identifier: identifier.clone(),
        }
    }
}

impl<S> From<(FxHashMap<String, Secret<serde_json::Value, S>>, Identifier)> for EncryptDataRequest
where
    S: Strategy<serde_json::Value>,
{
    fn from(
        (map, identifier): (FxHashMap<String, Secret<serde_json::Value, S>>, Identifier),
    ) -> Self {
        let mut group = FxHashMap::default();
        for (key, value) in map.into_iter() {
            group.insert(
                key.clone(),
                DecryptedData(StrongSecret::new(
                    value.clone().expose().to_string().as_bytes().to_vec(),
                )),
            );
        }
        Self {
            data: DecryptedDataGroup(group),
            identifier,
        }
    }
}

impl<S> From<(FxHashMap<String, Secret<String, S>>, Identifier)> for EncryptDataRequest
where
    S: Strategy<String>,
{
    fn from((map, identifier): (FxHashMap<String, Secret<String, S>>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        for (key, value) in map.into_iter() {
            group.insert(
                key.clone(),
                DecryptedData(StrongSecret::new(
                    value.clone().expose().as_bytes().to_vec(),
                )),
            );
        }
        Self {
            data: DecryptedDataGroup(group),
            identifier,
        }
    }
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct DecryptedDataGroup(pub FxHashMap<String, DecryptedData>);

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptDataResponse {
    pub data: EncryptedDataGroup,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct EncryptedDataGroup(pub FxHashMap<String, EncryptedData>);
#[derive(Debug, Serialize, Deserialize)]
pub struct DecryptDataRequest {
    #[serde(flatten)]
    pub identifier: Identifier,
    pub data: EncryptedDataGroup,
}

impl<T, S> ForeignFrom<(FxHashMap<String, Secret<T, S>>, EncryptDataResponse)>
    for FxHashMap<String, Encryptable<Secret<T, S>>>
where
    T: Clone,
    S: Strategy<T> + Send,
{
    fn foreign_from(
        (masked_data, response): (FxHashMap<String, Secret<T, S>>, EncryptDataResponse),
    ) -> Self {
        let mut encrypted = Self::default();
        for (k, v) in response.data.0.iter() {
            masked_data.get(k).map(|inner| {
                encrypted.insert(
                    k.clone(),
                    Encryptable::new(inner.clone(), v.data.peek().clone().into()),
                )
            });
        }
        encrypted
    }
}

impl<T, S> ForeignFrom<(Secret<T, S>, EncryptDataResponse)> for Option<Encryptable<Secret<T, S>>>
where
    T: Clone,
    S: Strategy<T> + Send,
{
    fn foreign_from((masked_data, response): (Secret<T, S>, EncryptDataResponse)) -> Self {
        response
            .data
            .0
            .get(DEFAULT_KEY)
            .map(|ed| Encryptable::new(masked_data.clone(), ed.data.peek().clone().into()))
    }
}

pub trait DecryptedDataConversion<T: Clone, S: Strategy<T> + Send>: Sized {
    fn convert(
        value: &DecryptedData,
        encryption: Encryption,
    ) -> CustomResult<Self, errors::CryptoError>;
}

impl<S: Strategy<String> + Send> DecryptedDataConversion<String, S>
    for Encryptable<Secret<String, S>>
{
    fn convert(
        value: &DecryptedData,
        encryption: Encryption,
    ) -> CustomResult<Self, errors::CryptoError> {
        Ok(Self::new(
            Secret::new(
                String::from_utf8(value.clone().inner().peek().clone())
                    .change_context(errors::CryptoError::DecodingFailed)?,
            ),
            encryption.clone().into_inner(),
        ))
    }
}

impl<S: Strategy<serde_json::Value> + Send> DecryptedDataConversion<serde_json::Value, S>
    for Encryptable<Secret<serde_json::Value, S>>
{
    fn convert(
        value: &DecryptedData,
        encryption: Encryption,
    ) -> CustomResult<Self, errors::CryptoError> {
        Ok(Self::new(
            Secret::new(
                serde_json::from_slice(value.clone().inner().peek())
                    .change_context(errors::CryptoError::DecodingFailed)?,
            ),
            encryption.clone().into_inner(),
        ))
    }
}

impl<S: Strategy<Vec<u8>> + Send> DecryptedDataConversion<Vec<u8>, S>
    for Encryptable<Secret<Vec<u8>, S>>
{
    fn convert(
        value: &DecryptedData,
        encryption: Encryption,
    ) -> CustomResult<Self, errors::CryptoError> {
        Ok(Self::new(
            Secret::new(value.clone().inner().peek().clone()),
            encryption.clone().into_inner(),
        ))
    }
}

impl<T, S> ForeignTryFrom<(Encryption, DecryptDataResponse)> for Encryptable<Secret<T, S>>
where
    T: Clone,
    S: Strategy<T> + Send,
    Self: DecryptedDataConversion<T, S>,
{
    type Error = error_stack::Report<errors::CryptoError>;
    fn foreign_try_from(
        (encrypted_data, response): (Encryption, DecryptDataResponse),
    ) -> Result<Self, Self::Error> {
        match response.data.0.get(DEFAULT_KEY) {
            Some(data) => Self::convert(data, encrypted_data),
            None => Err(errors::CryptoError::DecodingFailed)?,
        }
    }
}

impl<T, S> ForeignTryFrom<(FxHashMap<String, Encryption>, DecryptDataResponse)>
    for FxHashMap<String, Encryptable<Secret<T, S>>>
where
    T: Clone,
    S: Strategy<T> + Send,
    Encryptable<Secret<T, S>>: DecryptedDataConversion<T, S>,
{
    type Error = error_stack::Report<errors::CryptoError>;
    fn foreign_try_from(
        (encrypted_data, response): (FxHashMap<String, Encryption>, DecryptDataResponse),
    ) -> Result<Self, Self::Error> {
        let mut decrypted = Self::default();
        for (k, v) in response.data.0.iter() {
            match encrypted_data.get(k) {
                Some(encrypted) => {
                    decrypted.insert(k.clone(), Encryptable::convert(v, encrypted.clone())?);
                }
                None => Err(errors::CryptoError::DecodingFailed)?,
            }
        }
        Ok(decrypted)
    }
}

impl From<(Encryption, Identifier)> for DecryptDataRequest {
    fn from((encryption, identifier): (Encryption, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        group.insert(
            DEFAULT_KEY.to_string(),
            EncryptedData {
                data: StrongSecret::new(encryption.clone().into_inner().expose()),
            },
        );
        Self {
            data: EncryptedDataGroup(group),
            identifier,
        }
    }
}

impl From<(FxHashMap<String, Encryption>, Identifier)> for DecryptDataRequest {
    fn from((map, identifier): (FxHashMap<String, Encryption>, Identifier)) -> Self {
        let mut group = FxHashMap::default();
        for (key, value) in map.into_iter() {
            group.insert(
                key,
                EncryptedData {
                    data: StrongSecret::new(value.clone().into_inner().expose()),
                },
            );
        }
        Self {
            data: EncryptedDataGroup(group),
            identifier,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecryptDataResponse {
    pub data: DecryptedDataGroup,
}

#[derive(Clone, Debug)]
pub struct DecryptedData(StrongSecret<Vec<u8>>);

impl Serialize for DecryptedData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let data = BASE64_ENGINE.encode(self.0.peek());
        serializer.serialize_str(&data)
    }
}

impl<'de> Deserialize<'de> for DecryptedData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DecryptedDataVisitor;

        impl<'de> Visitor<'de> for DecryptedDataVisitor {
            type Value = DecryptedData;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("string of the format {version}:{base64_encoded_data}'")
            }

            fn visit_str<E>(self, value: &str) -> Result<DecryptedData, E>
            where
                E: de::Error,
            {
                let dec_data = BASE64_ENGINE.decode(value).map_err(|err| {
                    let err = err.to_string();
                    E::invalid_value(Unexpected::Str(value), &err.as_str())
                })?;

                Ok(DecryptedData(dec_data.into()))
            }
        }

        deserializer.deserialize_str(DecryptedDataVisitor)
    }
}

impl DecryptedData {
    pub fn from_data(data: StrongSecret<Vec<u8>>) -> Self {
        Self(data)
    }

    pub fn inner(self) -> StrongSecret<Vec<u8>> {
        self.0
    }
}

#[derive(Debug)]
pub struct EncryptedData {
    pub data: StrongSecret<Vec<u8>>,
}

impl Serialize for EncryptedData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let data = String::from_utf8(self.data.peek().clone()).map_err(ser::Error::custom)?;
        serializer.serialize_str(data.as_str())
    }
}

impl<'de> Deserialize<'de> for EncryptedData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EncryptedDataVisitor;

        impl<'de> Visitor<'de> for EncryptedDataVisitor {
            type Value = EncryptedData;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("string of the format {version}:{base64_encoded_data}'")
            }

            fn visit_str<E>(self, value: &str) -> Result<EncryptedData, E>
            where
                E: de::Error,
            {
                Ok(EncryptedData {
                    data: StrongSecret::new(value.as_bytes().to_vec()),
                })
            }
        }

        deserializer.deserialize_str(EncryptedDataVisitor)
    }
}
