// Copyright (C) 2025, Rafael Caricio <rafael@caricio.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// <https://mozilla.org/MPL/2.0/>.
//
// SPDX-License-Identifier: MPL-2.0

use gst::glib;
use gst::prelude::*;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::*;
use once_cell::sync::Lazy;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "videopipesink",
        gst::DebugColorFlags::empty(),
        Some("Video Subprocess Pipe Sink Element"),
    )
});

// Plugin state
struct State {
    child_process: Option<Child>,
    video_info: Option<gst_video::VideoInfo>,
    cmd: String,
    stderr_thread: Option<thread::JoinHandle<()>>,
    stderr_rx: Option<mpsc::Receiver<String>>,
}

// Properties
#[derive(Debug, Clone)]
struct Settings {
    cmd: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { cmd: String::new() }
    }
}

pub struct VideoPipeSink {
    settings: Mutex<Settings>,
    state: Mutex<State>,
}

impl Default for VideoPipeSink {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            state: Mutex::new(State {
                child_process: None,
                video_info: None,
                cmd: String::new(),
                stderr_thread: None,
                stderr_rx: None,
            }),
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for VideoPipeSink {
    const NAME: &'static str = "VideoPipeSink";
    type Type = super::VideoPipeSink;
    type ParentType = gst_base::BaseSink;
}

impl ObjectImpl for VideoPipeSink {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
            vec![glib::ParamSpecString::builder("cmd")
                .nick("Command")
                .blurb("Shell command to run")
                .mutable_ready()
                .build()]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        match pspec.name() {
            "cmd" => {
                let mut settings = self.settings.lock().unwrap();
                settings.cmd = value.get().expect("type checked upstream");
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "cmd" => {
                let settings = self.settings.lock().unwrap();
                settings.cmd.to_value()
            }
            _ => unimplemented!(),
        }
    }
}

impl GstObjectImpl for VideoPipeSink {}

impl ElementImpl for VideoPipeSink {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: Lazy<gst::subclass::ElementMetadata> = Lazy::new(|| {
            gst::subclass::ElementMetadata::new(
                "Vide Pipe Sink",
                "Sink/Video",
                "Pipes raw video frames to a provided subprocess",
                "Rafael Caricio <rafael@caricio.com>",
            )
        });

        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: Lazy<Vec<gst::PadTemplate>> = Lazy::new(|| {
            let caps = gst_video::VideoCapsBuilder::new()
                .format_list(gst_video::VideoFormat::iter_raw())
                .build();

            let sink_pad_template = gst::PadTemplate::new(
                "sink",
                gst::PadDirection::Sink,
                gst::PadPresence::Always,
                &caps,
            )
            .unwrap();

            vec![sink_pad_template]
        });

        PAD_TEMPLATES.as_ref()
    }
}

impl BaseSinkImpl for VideoPipeSink {
    fn set_caps(&self, caps: &gst::Caps) -> Result<(), gst::LoggableError> {
        let mut state = self.state.lock().unwrap();
        let info = gst_video::VideoInfo::from_caps(caps)
            .map_err(|_| gst::loggable_error!(CAT, "Failed to parse caps"))?;

        state.video_info = Some(info);
        Ok(())
    }

    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let settings = self.settings.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if settings.cmd.is_empty() {
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["Command line not set"]
            ));
        }

        // Create command
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&settings.cmd)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                gst::error_msg!(
                    gst::ResourceError::Failed,
                    ["Failed to start process: {}", e]
                )
            })?;

        // Setup stderr monitoring
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();

        let stderr_thread = thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let _ = tx.send(line);
                }
            }
        });

        state.child_process = Some(child);
        state.stderr_thread = Some(stderr_thread);
        state.stderr_rx = Some(rx);
        state.cmd = settings.cmd.clone();

        gst::info!(CAT, imp = self, "Started");
        Ok(())
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        // Stop child process
        if let Some(mut child) = state.child_process.take() {
            // Send SIGHUP
            #[cfg(unix)]
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGHUP);
            }

            // Wait for process
            match child.wait() {
                Ok(status) => {
                    gst::info!(CAT, "Process exited with status {}", status);
                }
                Err(err) => {
                    gst::warning!(CAT, "Failed to wait for child process: {}", err);
                }
            }
        }

        // Drain stderr
        if let Some(rx) = state.stderr_rx.take() {
            while let Ok(line) = rx.try_recv() {
                gst::debug!(CAT, "Process stderr: {}", line);
            }
        }

        // Join stderr thread
        if let Some(thread) = state.stderr_thread.take() {
            thread.join().unwrap();
        }

        state.video_info = None;

        gst::info!(CAT, imp = self, "Stopped");
        Ok(())
    }

    fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
        let state = &mut self.state.lock().unwrap();

        // Get the running time when this buffer should be rendered
        // let base_time = self.obj().base_time().unwrap();
        // let clock = self.obj().clock();

        // if let Some(clock) = clock {
        //     // Calculate the running time when this buffer should be rendered
        //     let running_time = buffer
        //         .pts()
        //         .opt_add(base_time)
        //         .unwrap_or(gst::ClockTime::ZERO);

        //     gst::debug!(CAT, imp = self, "Waiting for running time: {}", running_time);

        //     // Wait until the running time is reached
        //     let (wait_result, _jitter) = clock.new_single_shot_id(running_time).wait();

        //     wait_result.map_err(|_| {
        //         gst::error!(CAT, imp = self, "Failed to wait for running time");
        //         gst::FlowError::Error
        //     })?;
        // }

        let _video_info = match state.video_info {
            Some(ref i) => i,
            None => return Err(gst::FlowError::NotNegotiated),
        };

        let child = match state.child_process {
            Some(ref mut c) => c,
            None => {
                gst::error!(CAT, imp = self, "Child process not started");
                return Err(gst::FlowError::Error)
            },
        };

        // Map buffer for reading
        let mapped_buffer = buffer.map_readable().map_err(|_| {
            gst::error!(CAT, imp = self, "Failed to map buffer readable");
            gst::FlowError::Error
        })?;

        // Write to stdin
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            gst::error!(CAT, imp = self, "Child process stdin closed");
            gst::FlowError::Error
        })?;

        stdin.write_all(&mapped_buffer).map_err(|_| {
            gst::error!(CAT, imp = self, "Failed to write to process stdin");
            gst::FlowError::Error
        })?;
        gst::info!(CAT, imp = self, "Wrote buffer to process stdin");

        // Check for stderr
        if let Some(ref rx) = state.stderr_rx {
            while let Ok(line) = rx.try_recv() {
                gst::debug!(CAT, imp = self, "Process stderr: {}", line);
            }
        }

        Ok(gst::FlowSuccess::Ok)
    }

    fn unlock(&self) -> Result<(), gst::ErrorMessage> {
        Ok(())
    }

    fn unlock_stop(&self) -> Result<(), gst::ErrorMessage> {
        Ok(())
    }
}
