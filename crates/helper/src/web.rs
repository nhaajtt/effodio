//! The one way the helper goes online: the model and tool downloads the person asks for, and the version check.
//! A connection that stalls gives up instead of holding the helper (and every search waiting behind it) forever.

use std::sync::OnceLock;
use std::time::Duration;

pub fn get(url: &str) -> ureq::Request {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(30))
                // Between two pieces of a download, not for the whole of it: big files take as long as they take.
                .timeout_read(Duration::from_secs(60))
                .user_agent("effodio")
                .build()
        })
        .get(url)
}
