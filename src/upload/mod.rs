mod common;
mod records;
// Named after what it does — sending a combat — inside the module that gathers
// the ladder features. Renaming the file would only move the confusion.
#[allow(clippy::module_inception)]
mod upload;

pub use records::Records;
pub use upload::Upload;
