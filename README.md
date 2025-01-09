# GStreamer Subprocess Pipe Plugin

A GStreamer plugin that creates a video sink element which pipes raw video frames to a subprocess command via stdin.

## Description

The `videopipesink` element accepts raw video frames and forwards them to a subprocess command specified via the `cmd` property.

Key features:
- Accepts any raw video format as input
- Runs a user-specified command as a subprocess
- Pipes raw video frames to subprocess stdin
- Captures and logs subprocess stderr
- Gracefully handles subprocess lifecycle (start/stop)

## Installation

Ensure you have the following dependencies installed:
- Rust compiler and Cargo
- GStreamer development files
- libc development files

Build the plugin:
```bash
cargo build --release
```

## Element Usage

The element can be used in GStreamer pipelines by specifying the `cmd` property:

```bash
# Basic example converting video to ffmpeg
GST_PLUGIN_PATH=$PWD/target/debug GST_DEBUG=videopipesink:4 \
gst-launch-1.0 videotestsrc is-live=true ! videoconvert ! video/x-raw,format=I420,framerate=30/1 ! \
 videopipesink cmd="ffmpeg -hide_banner -f rawvideo -pix_fmt yuv420p -s 320x240 -r 30 -i - -c:v libx264 -preset medium -movflags +faststart -f mp4 -y output.mp4"

# Process frames with a Python script
gst-launch-1.0 v4l2src ! videoconvert ! video/x-raw,format=RGB ! \
    videopipesink cmd="python3 process_frames.py"
```

### Element Properties

- `cmd` (string): Shell command that will receive raw video frames via stdin. Required.

### Supported Formats

The element accepts any raw video format supported by GStreamer's video conversion elements. Common formats include:
- RGB, BGR
- GRAY8, GRAY16_LE
- YUV variants (I420, NV12, etc.)

### Behavior

- Paces frame delivery according to video frame rate
- Sends SIGHUP and waits for subprocess to exit on pipeline stop
- Logs subprocess stderr output and final return code
- Propagates subprocess errors to the pipeline

## Debugging

Enable debug output to see subprocess stderr and element state:

```bash
GST_DEBUG=videopipesink:4 gst-launch-1.0 ...
```

## License

This project is licensed under the Mozilla Public License 2.0.