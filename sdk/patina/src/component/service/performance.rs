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
    boot_services::StandardBootServices,
    component::service::{Service, perf_timer::ArchTimerFunctionality},
    performance::{
        Measurement,
        error::Error,
        measurement::CallerIdentifier,
        record::{GenericPerformanceRecord, PerformanceRecordBuffer, known::KnownPerfId},
    },
    uefi_protocol::performance_measurement::PerfAttribute,
};

/// Service that records firmware performance measurements into the Firmware Basic Boot Performance Table (FBPT).
///
pub trait PerformanceMeasurement: Send + Sync {
    /// Returns the bitmask of enabled [`Measurement`]s.
    fn measurement_mask(&self) -> u32;

    /// Sets the bitmask of enabled [`Measurement`]s.
    fn set_measurement_mask(&self, mask: u32);

    /// Sets the running load-image count, typically restored from a HOB at startup.
    fn set_load_image_count(&self, count: u32);

    /// Initializes the performance records of the table, typically restored from a HOB at startup.
    fn set_perf_records(&self, perf_records: PerformanceRecordBuffer);

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

    /// Begins performance measurement of start image in core.
    fn perf_image_start_begin(&self, module_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::StartImage as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(module_handle),
            None,
            None,
            0,
            0,
            KnownPerfId::ModuleStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of start image in core.
    fn perf_image_start_end(&self, image_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::StartImage as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(image_handle),
            None,
            None,
            0,
            0,
            KnownPerfId::ModuleEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of load image in core.
    fn perf_load_image_begin(&self, module_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::LoadImage as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(module_handle),
            None,
            None,
            0,
            0,
            KnownPerfId::ModuleLoadImageStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of load image in core.
    fn perf_load_image_end(&self, module_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::LoadImage as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(module_handle),
            None,
            None,
            0,
            0,
            KnownPerfId::ModuleLoadImageEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of driver binding support in the core.
    fn perf_driver_binding_support_begin(&self, driver_binding_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingSupport as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(driver_binding_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbSupportStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of driver binding support in the core.
    fn perf_driver_binding_support_end(&self, driver_binding_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingSupport as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(driver_binding_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbSupportEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of driver binding start in the core.
    fn perf_driver_binding_start_begin(&self, driver_binding_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingStart as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(driver_binding_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of driver binding start in the core.
    fn perf_driver_binding_start_end(&self, driver_binding_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingStart as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(driver_binding_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbEnd.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Begins performance measurement of driver binding stop in the core.
    fn perf_driver_binding_stop_begin(&self, module_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingStop as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(module_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbStopStart.as_u16(),
            PerfAttribute::PerfEntry,
        );
    }

    /// Ends performance measurement of driver binding stop in the core.
    fn perf_driver_binding_stop_end(&self, module_handle: efi::Handle, controller_handle: efi::Handle) {
        if self.measurement_mask() & Measurement::DriverBindingStop as u32 == 0 {
            return;
        }
        let _ = self.create_measurement(
            CallerIdentifier::Handle(module_handle),
            None,
            None,
            0,
            controller_handle as usize,
            KnownPerfId::ModuleDbStopEnd.as_u16(),
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

    /// Adds a record that records the start time of a performance measurement.
    fn perf_start(&self, handle: efi::Handle, token: Option<&str>, module: Option<&str>, timestamp: u64) {
        let _ = self.create_measurement(
            CallerIdentifier::Handle(handle),
            None,
            token.or(module),
            timestamp,
            0,
            0,
            PerfAttribute::PerfStartEntry,
        );
    }

    /// Adds a record that records the end time of a performance measurement.
    fn perf_end(&self, handle: efi::Handle, token: Option<&str>, module: Option<&str>, timestamp: u64) {
        let _ = self.create_measurement(
            CallerIdentifier::Handle(handle),
            None,
            token.or(module),
            timestamp,
            0,
            0,
            PerfAttribute::PerfEndEntry,
        );
    }

    /// Adds a record that records the start time of a performance measurement with an identifier.
    fn perf_start_ex(
        &self,
        handle: efi::Handle,
        token: Option<&str>,
        module: Option<&str>,
        timestamp: u64,
        identifier: u32,
    ) {
        let _ = self.create_measurement(
            CallerIdentifier::Handle(handle),
            None,
            token.or(module),
            timestamp,
            0,
            identifier as u16,
            PerfAttribute::PerfStartEntry,
        );
    }

    /// Adds a record that records the end time of a performance measurement with an identifier.
    fn perf_end_ex(
        &self,
        handle: efi::Handle,
        token: Option<&str>,
        module: Option<&str>,
        timestamp: u64,
        identifier: u32,
    ) {
        let _ = self.create_measurement(
            CallerIdentifier::Handle(handle),
            None,
            token.or(module),
            timestamp,
            0,
            identifier as u16,
            PerfAttribute::PerfEndEntry,
        );
    }
}
