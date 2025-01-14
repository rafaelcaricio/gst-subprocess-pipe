use gst::prelude::*;
use gst::ElementFactory;

fn main() {
    // Initialize GStreamer
    gst::init().unwrap();

    // Create the elements
    let src = ElementFactory::make("videotestsrc", None).unwrap();
    let convert = ElementFactory::make("videoconvert", None).unwrap();
    let sink = ElementFactory::make("videopipesink", None).unwrap();

    // Set the properties
    src.set_property("is-live", &true).unwrap();
    sink.set_property("cmd", &"ffmpeg -hide_banner -f rawvideo -pix_fmt yuv420p -s 320x240 -r 30 -i - -c:v libx264 -preset medium -movflags +faststart -f mp4 -y output.mp4").unwrap();

    // Create the pipeline
    let pipeline = gst::Pipeline::new(None);

    // Add elements to the pipeline
    pipeline.add_many(&[&src, &convert, &sink]).unwrap();

    // Link the elements
    src.link(&convert).unwrap();
    convert.link(&sink).unwrap();

    // Start playing
    pipeline.set_state(gst::State::Playing).unwrap();

    // Wait until error or EOS
    let bus = pipeline.bus().unwrap();
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        match msg.view() {
            gst::MessageView::Eos(..) => break,
            gst::MessageView::Error(err) => {
                eprintln!(
                    "Error from {:?}: {} ({:?})",
                    msg.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => (),
        }
    }

    // Clean up
    pipeline.set_state(gst::State::Null).unwrap();
}
