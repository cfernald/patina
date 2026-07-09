//! A serial logger implementation for the `log` crate.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use crate::serial::{SerialIO, shared::SharedSerial};
use core::marker::Send;

use super::Format;

/// A Base implementation for a logger.
///
/// ## Functionality
///
/// This implementation writes log messages directly to hardware port
///
pub struct Logger<'a, S>
where
    S: SerialIO + Send,
{
    serial_port: SharedSerial<S>,
    target_filters: &'a [(&'a str, log::LevelFilter)],
    max_level: log::LevelFilter,
    format: Format,
}

impl<'a, S> Logger<'a, S>
where
    S: SerialIO + Send,
{
    /// Creates a new logger instance.
    pub const fn new(
        format: Format,
        target_filters: &'a [(&'a str, log::LevelFilter)],
        max_level: log::LevelFilter,
        serial_port: S,
    ) -> Self {
        Self { serial_port: SharedSerial::new(serial_port), target_filters, max_level, format }
    }
}

impl<S> log::Log for Logger<'_, S>
where
    S: SerialIO + Send,
{
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level().to_level_filter()
            <= *self
                .target_filters
                .iter()
                .find(|(name, _)| metadata.target().starts_with(name))
                .map(|(_, level)| level)
                .unwrap_or(&self.max_level)
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let mut writer = LogWriter { serial_port: &self.serial_port };
            self.format.write(&mut writer, record);
        }
    }

    fn flush(&self) {
        // Do nothing
    }
}

/// A wrapper for handling log writes to a serial IO object.
struct LogWriter<'a, S>
where
    S: SerialIO + Send,
{
    serial_port: &'a SharedSerial<S>,
}

impl<S> core::fmt::Write for LogWriter<'_, S>
where
    S: SerialIO + Send,
{
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // Best-effort: serial write failures (e.g. contested access) must not break logging.
        let _ = self.serial_port.write(s.as_bytes());
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::log::Format;
    use crate::serial::MockSerialIO;
    use alloc::{string::String, sync::Arc, vec::Vec};
    use log::{Level, LevelFilter, Log, Metadata};
    use spin::Mutex;

    fn metadata(level: Level, target: &str) -> Metadata<'_> {
        Metadata::builder().level(level).target(target).build()
    }

    fn recording_serial() -> (MockSerialIO, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = buffer.clone();
        let mut mock = MockSerialIO::new();
        mock.expect_write().returning(move |bytes| sink.lock().extend_from_slice(bytes));
        (mock, buffer)
    }

    #[test]
    fn test_serial_logger_writes_using_format() {
        for (format, level, expected) in [
            (Format::Standard, Level::Info, "INFO - hello\r\n"),
            (Format::Json, Level::Warn, "{\"level\": \"WARN\" \"message\": \"hello\"}\r\n"),
        ] {
            let (mock, buffer) = recording_serial();
            let logger = Logger::new(format, &[], LevelFilter::Trace, mock);
            let args = format_args!("hello");
            logger.log(&log::Record::builder().args(args).level(level).target("test").build());
            assert_eq!(String::from_utf8(buffer.lock().clone()).expect("valid utf8"), expected);
        }
    }

    #[test]
    fn test_serial_logger_filters() {
        let logger = Logger::new(Format::Standard, &[], LevelFilter::Warn, MockSerialIO::new());
        assert!(logger.enabled(&metadata(Level::Error, "any")));
        assert!(logger.enabled(&metadata(Level::Warn, "any")));
        assert!(!logger.enabled(&metadata(Level::Info, "any")));

        let filters = &[("noisy::module", LevelFilter::Off), ("app", LevelFilter::Debug)];
        let logger = Logger::new(Format::Standard, filters, LevelFilter::Info, MockSerialIO::new());
        assert!(!logger.enabled(&metadata(Level::Error, "noisy::module::inner")));
        assert!(logger.enabled(&metadata(Level::Debug, "app::sub")));
        assert!(!logger.enabled(&metadata(Level::Trace, "app::sub")));
        assert!(logger.enabled(&metadata(Level::Info, "other")));
        assert!(!logger.enabled(&metadata(Level::Debug, "other")));
    }

    #[test]
    fn test_serial_logger_disabled_noop() {
        let mut mock = MockSerialIO::new();
        mock.expect_write().never();
        let logger = Logger::new(Format::Standard, &[], LevelFilter::Warn, mock);

        let args = format_args!("should be filtered out");
        logger.log(&log::Record::builder().args(args).level(Level::Info).target("test").build());
        logger.flush();
    }
}
