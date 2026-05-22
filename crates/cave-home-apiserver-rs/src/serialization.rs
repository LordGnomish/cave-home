// SPDX-License-Identifier: Apache-2.0
//! Wire-format codec.
//!
//! Source: kubernetes/kubernetes@756939600b9a7180fc2df6550a4585b638875e67
//! staging/src/k8s.io/apimachinery/pkg/runtime/serializer/json/json.go and
//! staging/src/k8s.io/apimachinery/pkg/runtime/serializer/yaml/yaml.go

use thiserror::Error;

use crate::api::ApiObject;
use crate::types::ContentType;

/// Codec error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CodecError {
    /// Body could not be parsed.
    #[error("invalid {0} body: {1}")]
    Invalid(&'static str, String),
}

/// Convenience alias.
pub type CodecResult<T> = Result<T, CodecError>;

/// Encode `obj` to the wire bytes for `ct`.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/runtime/serializer/json/json.go::Serializer::Encode
pub fn encode(obj: &ApiObject, ct: ContentType) -> CodecResult<Vec<u8>> {
    match ct {
        ContentType::Json => serde_json::to_vec(obj)
            .map_err(|e| CodecError::Invalid("json", e.to_string())),
        ContentType::Yaml => serde_yaml::to_string(obj)
            .map(String::into_bytes)
            .map_err(|e| CodecError::Invalid("yaml", e.to_string())),
    }
}

/// Decode an `ApiObject` from wire bytes.
///
/// Source: staging/src/k8s.io/apimachinery/pkg/runtime/serializer/json/json.go::Serializer::Decode
pub fn decode(bytes: &[u8], ct: ContentType) -> CodecResult<ApiObject> {
    match ct {
        ContentType::Json => serde_json::from_slice(bytes)
            .map_err(|e| CodecError::Invalid("json", e.to_string())),
        ContentType::Yaml => serde_yaml::from_slice(bytes)
            .map_err(|e| CodecError::Invalid("yaml", e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips() {
        let pod = ApiObject::new("v1", "Pod", "nginx");
        let bytes = encode(&pod, ContentType::Json).expect("encode");
        let back = decode(&bytes, ContentType::Json).expect("decode");
        assert_eq!(back.metadata.name, "nginx");
        assert_eq!(back.type_meta.kind, "Pod");
    }

    #[test]
    fn yaml_round_trips() {
        let pod = ApiObject::new("v1", "Pod", "nginx");
        let bytes = encode(&pod, ContentType::Yaml).expect("encode");
        let back = decode(&bytes, ContentType::Yaml).expect("decode");
        assert_eq!(back.metadata.name, "nginx");
    }

    #[test]
    fn decode_rejects_malformed_json() {
        let res = decode(b"{not-json}", ContentType::Json);
        assert!(matches!(res, Err(CodecError::Invalid(_, _))));
    }

    #[test]
    fn decode_rejects_malformed_yaml() {
        let res = decode(b"\t\t\tkey: [unclosed", ContentType::Yaml);
        assert!(matches!(res, Err(CodecError::Invalid(_, _))));
    }
}
