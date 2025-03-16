use std::path::PathBuf;
use gst::prelude::*;

fn main() {
    // Initialize GStreamer
    gst::init().unwrap();
    
    // Load the plugin
    load_plugin();
    
    // Create a simple pipeline
    let pipeline = gst::Pipeline::new();
    
    // Create a test source that produces data
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 30i32); // Only generate 30 frames
    
    // Create our videopipesink element
    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");
    
    // Set command to pipe output to standard output
    sink.set_property("cmd", "cat > /dev/null");
    
    // Add elements to pipeline
    pipeline.add_many(&[&src, &sink]).unwrap();
    
    // Link elements
    src.link(&sink).expect("Failed to link elements");
    
    // Start the pipeline
    println!("Starting pipeline...");
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");
    
    // Wait until EOS
    let bus = pipeline.bus().unwrap();
    let msg = bus.timed_pop_filtered(
        gst::ClockTime::from_seconds(10),
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );
    
    // Analyze message
    if let Some(msg) = msg {
        match msg.view() {
            gst::MessageView::Eos(..) => println!("End of stream"),
            gst::MessageView::Error(err) => {
                eprintln!(
                    "Error from {:?}: {} ({})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug().unwrap_or_default()
                );
            }
            _ => unreachable!(),
        }
    } else {
        println!("No message received, pipeline may still be running");
    }
    
    // Stop the pipeline
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");
    println!("Pipeline stopped");
}

fn load_plugin() {
    // Load the plugin
    let plugin = gst::Plugin::load_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(if cfg!(debug_assertions) { "debug" } else { "release" })
            .join(if cfg!(target_os = "windows") {
                "libgstsubprocesspipe.dll"
            } else if cfg!(target_os = "macos") {
                "libgstsubprocesspipe.dylib"
            } else {
                "libgstsubprocesspipe.so"
            }),
    ).expect("Failed to load plugin");
    
    // Register the plugin
    gst::Registry::get().add_plugin(&plugin).unwrap();
}
