use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn from_report(kind: &'static str, report: impl std::fmt::Display) -> Self {
        let message = report.to_string();
        let lower = message.to_lowercase();
        let kind = if lower.contains("-22406") || lower.contains("incorrect") && lower.contains("password") {
            "badPassword"
        } else if lower.contains("maximum number of app ids") || lower.contains("9401") || lower.contains("app id limit") {
            "appIdLimit"
        } else if lower.contains("maximum number of apps") || lower.contains("3 apps") {
            "appLimit"
        } else if lower.contains("devicelocked") || lower.contains("passwordprotected") {
            "deviceLocked"
        } else if lower.contains("error sending request") || lower.contains("dns") || lower.contains("timed out") {
            "network"
        } else {
            kind
        };
        Self { kind, message }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

pub type AppResult<T> = Result<T, AppError>;
