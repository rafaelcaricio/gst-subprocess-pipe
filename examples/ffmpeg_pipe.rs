use std::path::PathBuf;

use gst::prelude::*;

fn main() {
    // Initialize GStreamer
    gst::init().unwrap();
    
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
    gst::Registry::get().add_plugin(&plugin).expect("Failed to register plugin");

    // Create a simple pipeline
    let pipeline = gst::Pipeline::new();
    pipeline.set_property("name", "ffmpeg-pipe-test");
    
    // Create a video test source
    let src = gst::ElementFactory::make("videotestsrc")
        .name("source")
        .property("pattern", 18) // Ball pattern
        .property("is-live", false)
        .property("num-buffers", 300) // Generate 300 frames then send EOS (10 sec at 30fps)
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
    
    // Set caps to I420 format, which ffmpeg can handle
    let width = 640;
    let height = 480;
    let framerate = 30;
    
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "I420")
        .field("width", width)
        .field("height", height)
        .field("framerate", gst::Fraction::new(framerate, 1))
        .build();
    filter.set_property("caps", caps);
    
    // Create output file path
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("example_output.mp4");
    
    // Create ffmpeg command that takes raw I420 input and encodes to H.264 MP4
    let ffmpeg_cmd = format!(
        "ffmpeg -y -f rawvideo -pix_fmt yuv420p -s {}x{} -r {} -i - \
         -c:v libx264 -preset fast -crf 22 -f mp4 {}",
        width, height, framerate, output_path.display()
    );
    
    println!("Using ffmpeg command: {}", ffmpeg_cmd);
    
    // Create our videopipesink
    let sink = gst::ElementFactory::make("videopipesink")
        .name("sink")
        .property("cmd", ffmpeg_cmd)
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
    
    // Create a progress indicator
    let bus = pipeline.bus().unwrap();
    let mut done = false;
    let mut count = 0;
    
    println!("Processing frames...");
    
    while !done {
        let msg = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(1),
            &[gst::MessageType::Error, gst::MessageType::Eos],
        );
        
        match msg {
            Some(msg) => {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        println!("\nEnd of stream");
                        done = true;
                    }
                    gst::MessageView::Error(err) => {
                        println!(
                            "\nError from {:?}: {} ({})",
                            err.src().map(|s| s.path_string()),
                            err.error(),
                            err.debug().unwrap_or_default()
                        );
                        done = true;
                    }
                    _ => unreachable!(),
                }
            }
            None => {
                // No message, update progress indicator
                print!("\rProcessing... [{}/300 frames]", count);
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
                count += 30; // Approximate for 1 second at 30fps
                if count > 300 {
                    count = 300;
                }
            }
        }
    }
    
    // Clean up
    print!("\nStopping pipeline...");
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");
    println!(" done");
    
    // Print the output path for user reference
    println!("Output saved to: {}", output_path.display());
    println!("You can play this file with: ffplay -i {}", output_path.display());
}
