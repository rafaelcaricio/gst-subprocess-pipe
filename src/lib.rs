mod videopipesink;

use gst::glib;

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    videopipesink::register(plugin)
}

gst::plugin_define!(
    subprocesspipe,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MPL",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);
