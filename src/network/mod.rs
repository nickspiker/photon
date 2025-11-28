pub mod fgtw;
#[cfg(not(target_os = "android"))]
pub mod handle_query;
#[cfg(not(target_os = "android"))]
pub mod status;

#[cfg(not(target_os = "android"))]
pub use handle_query::{HandleQuery, QueryResult, RefreshResult};
#[cfg(not(target_os = "android"))]
pub use status::{StatusChecker, StatusUpdate};
