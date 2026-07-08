//! Performance measurement service interface.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use r_efi::efi;

use crate::{
    performance::{
        error::Error,
        measurement::CallerIdentifier,
        record::{GenericPerformanceRecord, known::KnownPerfId},
    },
    uefi_protocol::performance_measurement::PerfAttribute,
};

/// Service that records firmware performance measurements into the Firmware Basic Boot Performance Table (FBPT).
///
pub trait PerformanceMeasurement: Send + Sync {
    /// Function to log performance record with event description and a timestamp.
    #[allow(clippy::too_many_arguments)]
    fn create_measurement(
        &self,
        caller_identifier: CallerIdentifier,
        guid: Option<&efi::Guid>,
        string: Option<&str>,
        ticker: u64,
        address: usize,
        perf_id: u16,
        attribute: PerfAttribute,
    ) -> Result<(), Error>;

    /// Adds an already-formed generic performance record to the FBPT.
    fn add_generic_record(&self, record: GenericPerformanceRecord<&[u8]>) -> Result<(), Error>;

    /// Returns the number of bytes that must be allocated to publish the Firmware Basic Boot Performance Table.
    fn published_table_size(&self) -> Result<usize, Error>;

    /// Serializes the tracked records into the caller-provided `buffer` and puts the table into a published state.
    fn publish_table(&self, buffer: &'static mut [u8]) -> Result<(), Error>;

    /// Begins performance measurement of a behavior in different modules.
    fn perf_cross_module_begin(&self, measurement_str: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(measurement_str),
            0,
            0,
            KnownPerfId::PerfCrossModuleStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of a behavior in different modules.
    fn perf_cross_module_end(&self, measurement_str: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(measurement_str),
            0,
            0,
            KnownPerfId::PerfCrossModuleEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Measure the time from power-on to this function execution.
    fn perf_event(&self, event_string: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(event_string),
            0,
            0,
            KnownPerfId::PerfEvent.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of event signal behavior in any module.
    fn perf_event_signal_begin(&self, event_guid: &efi::Guid, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            Some(event_guid),
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfEventSignalStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of event signal behavior in any module.
    fn perf_event_signal_end(&self, event_guid: &efi::Guid, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            Some(event_guid),
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfEventSignalEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of a callback function in any module.
    fn perf_callback_begin(&self, trigger_guid: &efi::Guid, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            Some(trigger_guid),
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfCallbackStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of a callback function in any module.
    fn perf_callback_end(&self, trigger_guid: &efi::Guid, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            Some(trigger_guid),
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfCallbackEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begin performance measurement of any function in any module.
    fn perf_function_begin(&self, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfFunctionStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of any function in any module.
    fn perf_function_end(&self, fun_name: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(fun_name),
            0,
            0,
            KnownPerfId::PerfFunctionEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begin performance measurement of a behavior within one module.
    fn perf_in_module_begin(&self, measurement_str: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(measurement_str),
            0,
            0,
            KnownPerfId::PerfInModuleStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of a behavior within one module.
    fn perf_in_module_end(&self, measurement_str: &str, caller_id: &efi::Guid) {
        let _ = self.create_measurement(
            CallerIdentifier::Guid(*caller_id),
            None,
            Some(measurement_str),
            0,
            0,
            KnownPerfId::PerfInModuleEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }
}
