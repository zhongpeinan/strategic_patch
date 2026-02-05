use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    #[error("invalid JSON document: {0}")]
    BadJsonDoc(String),

    #[error("invalid argument type: expected {expected}, got {actual}")]
    BadArgType { expected: String, actual: String },

    #[error("invalid argument kind: expected {expected}, got {actual}")]
    BadArgKind { expected: String, actual: String },

    #[error("merge key '{merge_key}' not found in map at {path}")]
    NoMergeKey { path: String, merge_key: String },

    #[error("lists of lists are not supported")]
    NoListOfLists,

    #[error("invalid $patch directive: {0}")]
    BadPatchType(String),

    #[error("invalid patch format for primitive list at {path}")]
    BadPatchFormatForPrimitiveList { path: String },

    #[error("invalid patch format for setElementOrder at {path}")]
    BadPatchFormatForSetElementOrderList { path: String },

    #[error("invalid patch format for retainKeys at {path}")]
    BadPatchFormatForRetainKeys { path: String },

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("conflict: patch={patch}, current={current}")]
    Conflict { patch: String, current: String },

    #[error("strategic merge patch not supported for this type")]
    UnsupportedStrategicMergePatchFormat,

    #[error("field '{field}' not found in '{path}'")]
    FieldNotFound { path: String, field: String },

    #[error("invalid type: expected {expected}, got {actual}")]
    InvalidType { expected: String, actual: String },

    #[error("inconsistent list element types")]
    InconsistentListElementTypes,

    #[error("JSON error: {0}")]
    Json(String),
}

impl Error {
    pub fn http_status(&self) -> u16 {
        match self {
            Error::BadJsonDoc(_)
            | Error::BadArgType { .. }
            | Error::BadArgKind { .. }
            | Error::BadPatchType(_)
            | Error::BadPatchFormatForPrimitiveList { .. }
            | Error::BadPatchFormatForSetElementOrderList { .. }
            | Error::BadPatchFormatForRetainKeys { .. }
            | Error::PreconditionFailed(_)
            | Error::Json(_) => 400,
            Error::NoMergeKey { .. }
            | Error::NoListOfLists
            | Error::FieldNotFound { .. }
            | Error::InvalidType { .. }
            | Error::InconsistentListElementTypes => 422,
            Error::UnsupportedStrategicMergePatchFormat => 415,
            Error::Conflict { .. } => 409,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
