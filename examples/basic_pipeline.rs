use std::path::PathBuf;

use gst::prelude::*;

fn main() {
    // Initialize GStreamer
    gst::init().unwrap();
    
    // Register the element directly
    gstsubprocesspipe::register_element().unwrap();

    // Create a simple pipeline
    let pipeline = gst::Pipeline::new();
    pipeline.set_property("name", "test-pipeline");
    
    // Create a video test source
    let src = gst::ElementFactory::make("videotestsrc")
        .name("source")
        .property("pattern", 0) // Black and white checkerboard pattern
        .property("is-live", false)
        .property("num-buffers", 100) // Generate 100 frames then send EOS
        .build()
        .expect("Failed to create videotestsrc");
    
    // Convert video to a suitable format for encoding
    let convert = gst::ElementFactory::make("videoconvert")
        .name("convert")
        .build()
        .expect("Failed to create videoconvert");
    
    // Add a capsfilter to ensure a specific format
    let filter = gst::ElementFactory::make("capsfilter")
        .name("filter")
        .build()
        .expect("Failed to create capsfilter");
    
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "I420")
        .field("width", 320i32)
        .field("height", 240i32)
        .field("framerate", gst::Fraction::new(30, 1))
        .build();
    filter.set_property("caps", caps);
    
    // Create our videopipesink
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("example_output.raw");
    
    let sink = gst::ElementFactory::make("videopipesink")
        .name("sink")
        .property("cmd", format!("cat > {}", output_path.display()))
        .build()
        .expect("Failed to create videopipesink");
    
    // Add all elements to the pipeline
    pipeline.add_many(&[&src, &convert, &filter, &sink])
        .expect("Failed to add elements to pipeline");
    
    // Link the elements
    gst::Element::link_many(&[&src, &convert, &filter, &sink])
        .expect("Failed to link elements");
    
    // Set the pipeline to the playing state
    println!("Starting pipeline...");
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");
    
    // Wait until error or EOS
    let bus = pipeline.bus().unwrap();
    let mut done = false;
    
    while !done {
        let msg = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(1),
            &[gst::MessageType::Error, gst::MessageType::Eos, gst::MessageType::StateChanged],
        );
        
        match msg {
            Some(msg) => {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        println!("End of stream");
                        done = true;
                    }
                    gst::MessageView::Error(err) => {
                        println!(
                            "Error from {:?}: {} ({})",
                            err.src().map(|s| s.path_string()),
                            err.error(),
                            err.debug().unwrap_or_default()
                        );
                        done = true;
                    }
                    gst::MessageView::StateChanged(state) => {
                        if state.src().map(|s| s.name() == pipeline.name()).unwrap_or(false) {
                            println!(
                                "Pipeline state changed from {:?} to {:?}",
                                state.old(),
                                state.current()
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
            None => {
                // No message, just continue
            }
        }
    }
    
    // Clean up
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");
    println!("Pipeline stopped");
    
    // Print the output path for user reference
    println!("Output saved to: {}", output_path.display());
}
