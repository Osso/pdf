use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    InvalidArgs(String),

    #[error("{0}")]
    PdfInvalid(String),

    #[error("pdfium library not found: {0}")]
    PdfiumNotFound(String),

    #[error("rendering error: {0}")]
    Render(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Error::InvalidArgs(_) => ExitCode::from(1),
            Error::PdfInvalid(_) => ExitCode::from(2),
            Error::PdfiumNotFound(_) => ExitCode::from(3),
            Error::Render(_) => ExitCode::from(4),
            Error::Io(_) => ExitCode::from(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_distinguish_failure_classes() {
        assert_eq!(
            Error::InvalidArgs("bad".into()).exit_code(),
            ExitCode::from(1)
        );
        assert_eq!(
            Error::PdfInvalid("bad".into()).exit_code(),
            ExitCode::from(2)
        );
        assert_eq!(
            Error::PdfiumNotFound("missing".into()).exit_code(),
            ExitCode::from(3)
        );
        assert_eq!(
            Error::Render("failed".into()).exit_code(),
            ExitCode::from(4)
        );
        assert_eq!(
            Error::Io(std::io::Error::from(std::io::ErrorKind::Other)).exit_code(),
            ExitCode::from(5)
        );
    }
}
