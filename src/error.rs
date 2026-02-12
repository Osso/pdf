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
