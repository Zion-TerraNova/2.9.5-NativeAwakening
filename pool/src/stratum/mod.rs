pub mod protocol;
pub mod connection;

// V2 enhanced modules (server_v2 is primary)
pub mod server_v2;
pub mod connection_v2;

// Re-exports
pub use server_v2::StratumServer;
pub use connection_v2::{Connection, ConnectionState, Protocol};
pub use protocol::{
    StratumRequest, StratumResponse, StratumError,
    XMRigJob, ShareSubmission
};


