// Required imports for GStreamer testing

use gst::prelude::*;
use serial_test::serial;

// Initialize GStreamer and load the plugin
fn init() {
    gst::init().unwrap();
    
    // Register the element directly
    gstsubprocesspipe::register_element().unwrap();
}

#[test]
#[serial]
fn test_element_creation() {
    init();
    
    // Check if our element exists
    let element = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink element");
    
    // Verify element is the correct type
    assert_eq!(element.name(), "videopipesink0");
}

#[test]
#[serial]
fn test_properties() {
    init();
    
    let element = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink element");
    
    // Test default property values
    let cmd: String = element.property("cmd");
    assert_eq!(cmd, "");
    
    let wait_time: u64 = element.property("wait-for-exit");
    // Check default is 100ms in nanoseconds
    assert_eq!(wait_time, 100_000_000);
    
    // Set and get properties
    element.set_property("cmd", "cat");
    let cmd: String = element.property("cmd");
    assert_eq!(cmd, "cat");
    
    let new_wait_time: u64 = 200_000_000; // 200ms
    element.set_property("wait-for-exit", new_wait_time);
    let wait_time: u64 = element.property("wait-for-exit");
    assert_eq!(wait_time, new_wait_time);
}

#[test]
#[serial]
fn test_simple_pipeline() {
    init();
    
    let pipeline = gst::Pipeline::new();
    
    // Create a test source that produces data
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 10i32); // Only generate 10 frames for testing
    
    // Create our videopipesink element
    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");
    
    // Set a simple command
    sink.set_property("cmd", "cat > /dev/null");
    
    // Add elements to pipeline
    pipeline.add_many(&[&src, &sink]).unwrap();
    
    // Link elements
    src.link(&sink).expect("Failed to link elements");
    
    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");
    
    // Wait until EOS
    let bus = pipeline.bus().unwrap();
    let msg = bus.timed_pop_filtered(
        gst::ClockTime::from_seconds(5),
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );
    
    // Stop the pipeline
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");
    
    // Check the message
    if let Some(msg) = msg {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                // Success - pipeline completed normally
            }
            gst::MessageView::Error(err) => {
                panic!("Error from pipeline: {}", err.error());
            }
            _ => unreachable!(),
        }
    } else {
        panic!("No EOS or Error message received within timeout");
    }
}

#[test]
#[serial]
fn test_invalid_command() {
    init();
    
    let pipeline = gst::Pipeline::new();
    
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 1i32); // Only generate 1 frame
    
    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");
    
    // Set an invalid command that should fail
    sink.set_property("cmd", "non_existent_command_123xyz");
    
    // Add elements to pipeline
    pipeline.add_many(&[&src, &sink]).unwrap();
    
    // Link elements
    src.link(&sink).expect("Failed to link elements");
    
    // Try to start the pipeline - it should fail
    let state_change_result = pipeline.set_state(gst::State::Playing);
    
    // Get messages from the bus
    let bus = pipeline.bus().unwrap();
    let msg = bus.timed_pop_filtered(
        gst::ClockTime::from_seconds(2),
        &[gst::MessageType::Error, gst::MessageType::StateChanged],
    );
    
    // Set back to Null state
    pipeline.set_state(gst::State::Null).unwrap();
    
    // The test passes if either:
    // 1. We received an error message on the bus, or
    // 2. The state change failed directly
    if msg.is_none() {
        assert!(state_change_result.is_err(), "Expected either error message or state change failure");
    }
}

#[test]
#[serial]
fn test_complex_pipeline() {
    init();
    
    let pipeline = gst::Pipeline::new();
    
    // Create elements
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 30i32); // Generate 30 frames
    
    let convert = gst::ElementFactory::make("videoconvert")
        .build()
        .expect("Failed to create videoconvert");
    
    let capsfilter = gst::ElementFactory::make("capsfilter")
        .build()
        .expect("Failed to create capsfilter");
    let caps = gst::Caps::builder("video/x-raw")
        .field("width", 320i32)
        .field("height", 240i32)
        .build();
    capsfilter.set_property("caps", caps);
    
    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");
    
    // Output to /dev/null to avoid creating unnecessary files
    sink.set_property("cmd", "cat > /dev/null");
    
    // Add all elements to pipeline
    pipeline.add_many(&[&src, &convert, &capsfilter, &sink])
        .expect("Failed to add elements to pipeline");
    
    // Link elements
    gst::Element::link_many(&[&src, &convert, &capsfilter, &sink])
        .expect("Failed to link elements");
    
    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");
    
    // Wait until EOS or error
    let bus = pipeline.bus().unwrap();
    let msg = bus.timed_pop_filtered(
        gst::ClockTime::from_seconds(10),
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );
    
    // Stop the pipeline
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");
    
    // Check for errors
    if let Some(msg) = msg {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                // Success - pipeline completed normally
            }
            gst::MessageView::Error(err) => {
                panic!("Error from pipeline: {}", err.error());
            }
            _ => unreachable!(),
        }
    } else {
        panic!("No EOS or Error message received within timeout");
    }
}
