use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

pub type StateResult<T> = Result<T, ProjectStateError>;

/// Backend-neutral state failure categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectStateError {
    NotFound { message: String },
    Conflict { message: String },
    Unsupported { capability: String, message: String },
    InvalidCommand { message: String },
    Serialization { message: String },
    Backend { message: String },
}

impl ProjectStateError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    pub fn unsupported(capability: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Unsupported {
            capability: capability.into(),
            message: message.into(),
        }
    }

    pub fn invalid_command(message: impl Into<String>) -> Self {
        Self::InvalidCommand {
            message: message.into(),
        }
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::Backend {
            message: message.into(),
        }
    }
}

impl Display for ProjectStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { message }
            | Self::Conflict { message }
            | Self::InvalidCommand { message }
            | Self::Serialization { message }
            | Self::Backend { message } => write!(formatter, "{message}"),
            Self::Unsupported {
                capability,
                message,
            } => {
                write!(formatter, "{capability}: {message}")
            }
        }
    }
}

impl Error for ProjectStateError {}

impl From<serde_json::Error> for ProjectStateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_backend_neutral_errors() {
        let error = ProjectStateError::unsupported(
            "shared_coordination",
            "backend only supports local coordination",
        );

        assert_eq!(
            error.to_string(),
            "shared_coordination: backend only supports local coordination"
        );
    }
}
