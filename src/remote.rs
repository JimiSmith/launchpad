//! UI-side worker demand: one replaceable latest query, never a catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRequest {
    Query { generation: u64, text: String },
    Validate { generation: u64, raw: String },
}
impl RemoteRequest {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Query { generation, .. } | Self::Validate { generation, .. } => *generation,
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct Remote {
    pub refresh: u64,
    pub generation: u64,
    pub revision: u64,
    pub dirty: bool,
    pub validation: Option<Validation>,
    pub outbound: Option<RemoteRequest>,
    pub failed: bool,
}
#[derive(Debug)]
pub(crate) enum Validation {
    Accept,
    Launch(crate::app::Tool),
}
