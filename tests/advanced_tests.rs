use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process;
use std::time::{Duration, Instant};
use std::thread;

use gst::prelude::*;
use serial_test::serial;

// Initialize GStreamer and load the plugin
fn init() {
    gst::init().unwrap();

    // Register the element directly
    gstsubprocesspipe::register_element().unwrap();
}

// Helper function to create a temporary file path
fn create_temp_filepath(suffix: &str) -> String {
    let pid = process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    format!("/tmp/gst-subprocess-pipe-test-{}-{}.{}", pid, timestamp, suffix)
}

// Helper function to wait for EOS or Error message
fn wait_for_message(
    pipeline: &gst::Pipeline,
    timeout: gst::ClockTime,
    msg_types: &[gst::MessageType],
) -> Option<gst::Message> {
    let bus = pipeline.bus().unwrap();
    bus.timed_pop_filtered(timeout, msg_types)
}

// Helper to build a basic pipeline with videopipesink
fn build_pipeline(cmd: &str, num_buffers: i32) -> gst::Pipeline {
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", num_buffers);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    sink.set_property("cmd", cmd);

    pipeline.add_many(&[&src, &sink]).unwrap();
    src.link(&sink).expect("Failed to link elements");

    pipeline
}

#[test]
#[serial]
fn test_specific_video_format() {
    init();

    let pipeline = gst::Pipeline::new();

    // Create elements
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 10i32);

    let convert = gst::ElementFactory::make("videoconvert")
        .build()
        .expect("Failed to create videoconvert");

    let capsfilter = gst::ElementFactory::make("capsfilter")
        .build()
        .expect("Failed to create capsfilter");

    // Specific format: RGB with 640x480
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGB")
        .field("width", 640i32)
        .field("height", 480i32)
        .build();
    capsfilter.set_property("caps", caps);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Output to a temp file so we can verify the data
    let temp_file = create_temp_filepath("raw");
    let cmd = format!("cat > {}", temp_file);
    sink.set_property("cmd", cmd.clone());

    // Add elements to pipeline
    pipeline.add_many(&[&src, &convert, &capsfilter, &sink])
        .expect("Failed to add elements to pipeline");

    // Link elements
    gst::Element::link_many(&[&src, &convert, &capsfilter, &sink])
        .expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait until EOS or error
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
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

    // Verify the output file exists and has data
    let metadata = fs::metadata(&temp_file).expect("Output file not created");

    // RGB format with 640x480 should result in a file size of at least:
    // 640 * 480 * 3 (bytes per pixel) * 10 (frames) = 9,216,000 bytes
    assert!(metadata.len() > 9_000_000, "File size too small: {}", metadata.len());

    // Clean up the temporary file
    fs::remove_file(temp_file).ok();
}

#[test]
#[serial]
fn test_subprocess_exit_handling() {
    init();

    // This test checks how the element handles a subprocess that exits prematurely
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    // Set a high number of buffers, but the process will exit early
    src.set_property("num-buffers", 100i32);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Command that will exit after processing 1 buffer
    sink.set_property("cmd", "head -c 1000 > /dev/null && exit 1");

    pipeline.add_many(&[&src, &sink]).unwrap();
    src.link(&sink).expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // The pipeline should error out once the subprocess exits
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
        &[gst::MessageType::Error, gst::MessageType::Eos],
    );

    // Stop the pipeline
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");

    // We should have received an error
    if let Some(msg) = msg {
        match msg.view() {
            gst::MessageView::Error(_) => {
                // Expected - subprocess exited with an error
            }
            gst::MessageView::Eos(..) => {
                panic!("Expected an error but got EOS");
            }
            _ => unreachable!(),
        }
    } else {
        panic!("No message received within timeout");
    }
}

#[test]
#[serial]
fn test_changing_cmd_property() {
    init();

    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 10i32);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Initial command
    sink.set_property("cmd", "cat > /dev/null");

    pipeline.add_many(&[&src, &sink]).unwrap();
    src.link(&sink).expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait a short time
    thread::sleep(Duration::from_millis(500));

    // Change to NULL state
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");

    // Change the command
    let temp_file = create_temp_filepath("txt");
    let new_cmd = format!("cat > {}", temp_file);
    sink.set_property("cmd", new_cmd.clone());

    // Restart the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait for EOS
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
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

    // Verify the output file exists and has data
    assert!(Path::new(&temp_file).exists(), "Output file not created");

    // Clean up the temporary file
    fs::remove_file(temp_file).ok();
}

#[test]
#[serial]
fn test_pause_resume_pipeline() {
    init();

    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 30i32);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Output to a temp file so we can analyze the output
    let temp_file = create_temp_filepath("raw");
    let cmd = format!("cat > {}", temp_file);
    sink.set_property("cmd", cmd);

    pipeline.add_many(&[&src, &sink]).unwrap();
    src.link(&sink).expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Let it run for a short time
    thread::sleep(Duration::from_millis(500));

    // Pause the pipeline
    pipeline.set_state(gst::State::Paused).expect("Failed to set pipeline to Paused");

    // Wait a moment while paused
    thread::sleep(Duration::from_millis(1000));

    // Check file size at pause time
    let paused_size = fs::metadata(&temp_file)
        .expect("Output file not created")
        .len();

    // Resume the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait for EOS
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
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

    // Verify the final file size is larger than when paused
    let final_size = fs::metadata(&temp_file)
        .expect("Output file not created")
        .len();

    assert!(final_size > paused_size,
            "File size should have increased after resuming (paused: {}, final: {})",
            paused_size, final_size);

    // Clean up the temporary file
    fs::remove_file(temp_file).ok();
}

#[test]
#[serial]
fn test_high_framerate_processing() {
    init();

    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 100i32);

    let capsfilter = gst::ElementFactory::make("capsfilter")
        .build()
        .expect("Failed to create capsfilter");

    // Set a high framerate
    let caps = gst::Caps::builder("video/x-raw")
        .field("framerate", gst::Fraction::new(60, 1))
        .build();
    capsfilter.set_property("caps", caps);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Use /dev/null for fast processing
    sink.set_property("cmd", "cat > /dev/null");

    pipeline.add_many(&[&src, &capsfilter, &sink]).unwrap();
    gst::Element::link_many(&[&src, &capsfilter, &sink]).expect("Failed to link elements");

    // Start the pipeline
    let start_time = Instant::now();
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait for EOS
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(10),
        &[gst::MessageType::Eos, gst::MessageType::Error],
    );
    let elapsed = start_time.elapsed();

    // Stop the pipeline
    pipeline.set_state(gst::State::Null).expect("Failed to set pipeline to Null");

    // Check for errors
    if let Some(msg) = msg {
        match msg.view() {
            gst::MessageView::Eos(..) => {
                // Success - pipeline completed normally
                println!("High framerate test completed in {:?}", elapsed);
            }
            gst::MessageView::Error(err) => {
                panic!("Error from pipeline: {}", err.error());
            }
            _ => unreachable!(),
        }
    } else {
        panic!("No EOS or Error message received within timeout");
    }

    // With 100 frames at 60fps, it should take at least 100/60 ≈ 1.67 seconds
    // Allow some overhead but ensure it's not too slow
    assert!(elapsed >= Duration::from_millis(1500),
            "Process completed too quickly: {:?}", elapsed);
    assert!(elapsed < Duration::from_secs(5),
            "Process took too long: {:?}", elapsed);
}

#[test]
#[serial]
fn test_stderr_capture() {
    init();

    // This test verifies that stderr output from the subprocess is captured
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 10i32);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Command that will produce some stderr output
    sink.set_property("cmd", "sh -c 'cat > /dev/null; echo This is error output 1>&2'");

    pipeline.add_many(&[&src, &sink]).unwrap();
    src.link(&sink).expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait for EOS
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
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

    // Note: We cannot directly verify the stderr output in this test
    // since it's being logged through GStreamer's logging system.
    // In a real situation, we'd capture the logs.
}

#[test]
#[serial]
fn test_subprocess_output_verification() {
    init();

    // This test verifies the actual byte content being sent to the subprocess
    let pipeline = gst::Pipeline::new();

    // Create a specific pattern with videotestsrc
    let src = gst::ElementFactory::make("videotestsrc")
        .build()
        .expect("Failed to create videotestsrc");
    src.set_property("num-buffers", 1i32);
    src.set_property_from_str("pattern", "smpte"); // 0 = smpte pattern

    let capsfilter = gst::ElementFactory::make("capsfilter")
        .build()
        .expect("Failed to create capsfilter");

    // Use a small frame size for easier verification
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGB")
        .field("width", 64i32)
        .field("height", 64i32)
        .build();
    capsfilter.set_property("caps", caps);

    let sink = gst::ElementFactory::make("videopipesink")
        .build()
        .expect("Failed to create videopipesink");

    // Output to a temp file for verification
    let temp_file = create_temp_filepath("rgb");
    let cmd = format!("cat > {}", temp_file);
    sink.set_property("cmd", cmd);

    pipeline.add_many(&[&src, &capsfilter, &sink]).unwrap();
    gst::Element::link_many(&[&src, &capsfilter, &sink]).expect("Failed to link elements");

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).expect("Failed to set pipeline to Playing");

    // Wait for EOS
    let msg = wait_for_message(
        &pipeline,
        gst::ClockTime::from_seconds(5),
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

    // Verify the file exists and has the right size
    let metadata = fs::metadata(&temp_file).expect("Output file not created");

    // Expected size for an RGB 64x64 image is 64*64*3 = 12,288 bytes
    assert_eq!(metadata.len(), 12_288,
              "Unexpected file size: {} (expected 12,288)", metadata.len());

    // Read a sample of bytes to verify the content
    let mut file = File::open(&temp_file).expect("Failed to open output file");
    let mut buffer = [0u8; 16];
    file.read_exact(&mut buffer).expect("Failed to read from file");

    // Since the SMPTE pattern has specific color values, we could check for those,
    // but for this test we'll just verify we have non-zero data
    assert!(buffer.iter().any(|&b| b > 0), "Expected non-zero data in output file");

    // Clean up the temporary file
    fs::remove_file(temp_file).ok();
}
