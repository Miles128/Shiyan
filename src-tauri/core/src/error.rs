use serde::{Serialize, Serializer};

/// Unified application error, surfaced to the frontend as its display string.
/// Typed variants carry the underlying library error; user-facing messages
/// use [`AppError::msg`].
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error("数据源状态被占用，请稍后重试")]
    Locked,
    #[error("数据库错误：{0}")]
    Db(#[from] rusqlite::Error),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Msg(s.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Msg(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Msg(s.to_string())
    }
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

/// Unwrap `spawn_blocking(...).await` (join error + inner error).
pub fn flatten_blocking<T, E, J>(joined: Result<Result<T, E>, J>) -> Result<T, AppError>
where
    E: Into<AppError>,
    J: std::fmt::Display,
{
    joined
        .map_err(|e| AppError::from(e.to_string()))?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_serializes_as_display_string() {
        let json = serde_json::to_string(&AppError::Locked).unwrap();
        assert_eq!(json, "\"数据源状态被占用，请稍后重试\"");
    }
}
