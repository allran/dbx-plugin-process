mod icon;
mod local;
mod model;
mod plugin;
mod ports;
mod protected;
mod remote;
mod reveal;
mod threads;
mod util;

use dbx_plugin_sdk::{PluginMetadata, PluginServer};

use plugin::Plugin;
use util::log_stderr;

fn main() -> std::io::Result<()> {
    log_stderr("sidecar starting");
    let metadata = PluginMetadata::new("io.dbx.process", env!("CARGO_PKG_VERSION"))
        .with_capability("connections")
        .with_capability("events");
    PluginServer::new(metadata, Plugin::new()).serve()
}
