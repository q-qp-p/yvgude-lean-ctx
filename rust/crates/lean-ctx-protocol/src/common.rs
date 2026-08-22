//! Shared wire primitives used by the version-one protocol contracts.

use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use std::{error::Error, fmt};

/// The schema version implemented by this crate.
pub const V1_SCHEMA_VERSION: u32 = 1;

/// Maximum size for an opaque identifier on the wire.
pub const MAX_IDENTIFIER_LENGTH: usize = 256;

/// Validate an opaque wire identifier before it crosses a protocol boundary.
pub(crate) fn validate_bounded_opaque_identifier(
    value: &str,
    type_name: &str,
) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new(format!(
            "{type_name} must not be empty"
        )));
    }
    if value.len() > MAX_IDENTIFIER_LENGTH {
        return Err(ValidationError::new(format!(
            "{type_name} exceeds the {MAX_IDENTIFIER_LENGTH} byte limit"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(ValidationError::new(format!(
            "{type_name} must not contain control characters"
        )));
    }
    Ok(())
}

/// Error returned when a contract value cannot satisfy a wire invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(pub(crate) String);

impl ValidationError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ValidationError {}

macro_rules! bounded_identifier {
    ($name:ident) => {
        /// Opaque, bounded identifier used by a protocol contract.
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct an identifier after applying the wire bounds.
            pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
                let value = value.into();
                $crate::common::validate_bounded_opaque_identifier(&value, stringify!($name))?;
                Ok(Self(value))
            }

            /// Borrow the identifier's opaque wire value.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the identifier and return its wire value.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = ValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ValidationError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(DeError::custom)
            }
        }
    };
}

bounded_identifier!(TaskId);
bounded_identifier!(TraceId);
bounded_identifier!(ProjectId);
bounded_identifier!(SessionId);
bounded_identifier!(AgentId);
bounded_identifier!(TenantId);
bounded_identifier!(PlanId);
bounded_identifier!(ReceiptId);
bounded_identifier!(OutcomeId);
bounded_identifier!(CapabilityId);
bounded_identifier!(DecisionId);

/// Require the one schema version implemented by this crate during decoding.
pub fn deserialize_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    if version == V1_SCHEMA_VERSION {
        Ok(version)
    } else {
        Err(DeError::custom(format!(
            "unsupported schema_version {version}; expected {V1_SCHEMA_VERSION}"
        )))
    }
}

/// Decode a required milliunit constrained to the inclusive 0..=1000 range.
pub fn deserialize_milliunit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u16::deserialize(deserializer)?;
    if value <= 1000 {
        Ok(value)
    } else {
        Err(DeError::custom("milliunit must be between 0 and 1000"))
    }
}

/// Decode an optional milliunit constrained to the inclusive 0..=1000 range.
pub fn deserialize_optional_milliunit<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<u16>::deserialize(deserializer)?;
    value.map_or(Ok(None), |value| {
        if value <= 1000 {
            Ok(Some(value))
        } else {
            Err(DeError::custom("milliunit must be between 0 and 1000"))
        }
    })
}

/// Validate a schema version on values built directly in Rust.
pub fn validate_schema_version(version: u32) -> Result<(), ValidationError> {
    if version == V1_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ValidationError::new(format!(
            "unsupported schema_version {version}; expected {V1_SCHEMA_VERSION}"
        )))
    }
}

/// Validate a 0..=1000 milliunit value on values built directly in Rust.
pub fn validate_milliunit(value: u16, field: &str) -> Result<(), ValidationError> {
    if value <= 1000 {
        Ok(())
    } else {
        Err(ValidationError::new(format!(
            "{field} must be between 0 and 1000"
        )))
    }
}
