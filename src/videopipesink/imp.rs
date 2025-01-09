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
use std::sync::Mutex;
use std::thread;

static CAT: Lazy<gst::DebugCategory> = Lazy::new(|| {
    gst::DebugCategory::new(
        "videopipesink",
        gst::DebugColorFlags::empty(),
        Some("Video Subprocess Pipe Sink Element"),
    )
});

static WAIT_FOR_EXIT_DEFAULT: gst::ClockTime = gst::ClockTime::from_mseconds(100);

// Plugin state
struct State {
    child_process: Option<Child>,
    video_info: Option<gst_video::VideoInfo>,
    cmd: String,
    stdout_thread: Option<thread::JoinHandle<()>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
}

// Properties
#[derive(Debug, Clone)]
struct Settings {
    cmd: String,
    wait_for_exit: gst::ClockTime,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { 
            cmd: String::new(),
            wait_for_exit: WAIT_FOR_EXIT_DEFAULT,
         }
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
                stdout_thread: None,
                stderr_thread: None,
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
            vec![
                glib::ParamSpecString::builder("cmd")
                    .nick("Command")
                    .blurb("Shell command to run")
                    .mutable_ready()
                    .build(),
                glib::ParamSpecUInt64::builder("wait-for-exit")
                    .nick("Wait for exit")
                    .blurb("Wait time in nanoseconds for the subprocess to exit after the stdin pipe is closed")
                    .default_value(0)
                    .mutable_playing()
                    .build(),
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        let mut settings = self.settings.lock().unwrap();
        match pspec.name() {
            "cmd" => {
                settings.cmd = value.get().expect("type checked upstream");
            }
            "wait-for-exit" => {
                settings.wait_for_exit = value.get().expect("type checked upstream");
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        let settings = self.settings.lock().unwrap();
        match pspec.name() {
            "cmd" => {
                settings.cmd.to_value()
            }
            "wait-for-exit" => {
                settings.wait_for_exit.to_value()
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
        gst::debug!(CAT, imp = self, "Caps set to: {}", caps);
        Ok(())
    }

    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let settings = self.settings.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if settings.cmd.is_empty() {
            gst::debug!(CAT, imp = self, "Command line not set");
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["Command line not set"]
            ));
        }

        // Get current working directory
        let current_dir = std::env::current_dir().map_err(|e| {
            gst::error_msg!(
                gst::ResourceError::Failed,
                ["Failed to get current directory: {}", e]
            )
        })?;

        gst::info!(CAT, imp = self, "Starting subprocess with command: {}", settings.cmd);

        // Create command
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&settings.cmd)
            .current_dir(current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                gst::error_msg!(
                    gst::ResourceError::Failed,
                    ["Failed to start process: {}", e]
                )
            })?;

        let pid = child.id();

        // Setup stdout monitoring
        let stdout = child.stdout.take().unwrap();

        let stdout_thread = thread::spawn({
            let this = self.downgrade();
            move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stdout);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        let this = match this.upgrade() {
                            Some(this) => this,
                            None => return,
                        };
                        gst::debug!(CAT, imp = this, "stdout: {}", line);
                    }
                }
            }
        });

        // Setup stderr monitoring
        let stderr = child.stderr.take().unwrap();

        let stderr_thread = thread::spawn({
            let this = self.downgrade();
            move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        let this = match this.upgrade() {
                            Some(this) => this,
                            None => return,
                        };
                        gst::warning!(CAT, imp = this, "stderr: {}", line);
                    }
                }
            }
        });

        state.child_process = Some(child);
        state.stdout_thread = Some(stdout_thread);
        state.stderr_thread = Some(stderr_thread);
        state.cmd = settings.cmd.clone();

        gst::info!(CAT, imp = self, "Started subprocess with PID: {}", pid);
        Ok(())
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        // Stop child process
        if let Some(mut child) = state.child_process.take() {
            let pid = child.id();

            // Drop stdin to send EOF
            drop(child.stdin.take());

            let settings = self.settings.lock().unwrap();
            std::thread::sleep(settings.wait_for_exit.into());

            // Send SIGHUP
            #[cfg(unix)]
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGHUP);
            }

            // Wait for process
            match child.wait() {
                Ok(status) => {
                    if let Some(code) = status.code() {
                        gst::info!(CAT, imp = self, "Process (PID: {}) exited with code {}", pid, code);
                    } else {
                        gst::info!(CAT, imp = self, "Process (PID: {}) terminated by signal", pid);
                    }
                }
                Err(err) => {
                    gst::warning!(CAT, imp = self, "Failed to wait for child process (PID: {}): {}", pid, err);
                }
            }
        }

        // Join stdout and stderr threads
        if let Some(thread) = state.stdout_thread.take() {
            thread.join().unwrap();
        }

        if let Some(thread) = state.stderr_thread.take() {
            thread.join().unwrap();
        }

        state.video_info = None;

        gst::info!(CAT, imp = self, "Stopped");
        Ok(())
    }

    fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
        let mut state = self.state.lock().unwrap();

        let Some(_) = state.video_info else {
            gst::error!(CAT, imp = self, "Video info not set");
            return Err(gst::FlowError::NotNegotiated);
        };

        // Get child process and check if it's still running
        let child = match &mut state.child_process {
            Some(c) => {
                // Try to get status without waiting
                match c.try_wait() {
                    Ok(Some(status)) => {
                        let pid = c.id();
                        // Process has exited unexpectedly
                        gst::error!(CAT, imp = self, "Subprocess (PID: {}) exited unexpectedly", pid);

                        if let Some(code) = status.code() {
                            gst::error!(CAT, imp = self, "Exit code: {}", code);
                        } else {
                            gst::error!(CAT, imp = self, "Process terminated by signal");
                        }

                        return Err(gst::FlowError::Error);
                    }
                    Ok(None) => c, // Process still running
                    Err(e) => {
                        gst::error!(CAT, imp = self, "Failed to check subprocess status: {}", e);
                        return Err(gst::FlowError::Error);
                    }
                }
            }
            None => {
                gst::error!(CAT, imp = self, "Child process not started");
                return Err(gst::FlowError::Error);
            }
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

        // Write frame data
        match stdin.write_all(&mapped_buffer) {
            Ok(_) => {
                // Flush to ensure data is sent immediately
                if let Err(e) = stdin.flush() {
                    gst::error!(CAT, imp = self, "Failed to flush stdin: {}", e);
                    return Err(gst::FlowError::Error);
                }
                gst::trace!(CAT, imp = self, "Wrote and flushed buffer of size {}", mapped_buffer.size());
            }
            Err(e) => {
                gst::error!(CAT, imp = self, "Failed to write to process stdin: {}", e);
                return Err(gst::FlowError::Error);
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
