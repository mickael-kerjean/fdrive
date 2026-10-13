#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("permission denied")]
    PermissionDenied,
    #[error("not found")]
    NotFound,
    #[error("precondition failed")]
    PreconditionFailed,
    #[error("not supported by this server")]
    Unsupported,
    #[error("not a Filestash server")]
    NotFilestash,
    #[error("api error: {0}")]
    Api(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),
}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        match err {
            Error::NotFound => std::io::ErrorKind::NotFound.into(),
            Error::PermissionDenied => std::io::ErrorKind::PermissionDenied.into(),
            err => std::io::Error::other(err),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
