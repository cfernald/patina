//! Core performance measurement service.
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use core::{
    mem, ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use patina::{
    BinaryGuid,
    component::service::{IntoService, perf_timer::ArchTimerFunctionality, performance::PerformanceMeasurement},
    error::EfiError,
    performance::{
        error::Error,
        measurement::CallerIdentifier,
        record::{
            GenericPerformanceRecord, PerformanceRecordBuffer,
            extended::{
                DualGuidStringEventRecord, DynamicStringEventRecord, GuidEventRecord, GuidQwordEventRecord,
                GuidQwordStringEventRecord,
            },
            known::KnownPerfId,
        },
        table::{FBPT, FirmwareBasicBootPerfTable},
    },
    uefi_protocol::performance_measurement::PerfAttribute,
};

use crate::{cpu::PerfTimer, protocols::PROTOCOL_DB, tpl_mutex::TplMutex};

use r_efi::{
    efi,
    protocols::device_path::{Media, TYPE_MEDIA},
};

static LOAD_IMAGE_COUNT: AtomicU32 = AtomicU32::new(0);
static PERF_MEASUREMENT_MASK: AtomicU32 = AtomicU32::new(0);
/// Performance timer frequency in Hz. A value of `0` means the architecture-specific frequency is auto-detected.
static PERF_FREQUENCY: AtomicU64 = AtomicU64::new(0);

/// The Firmware Basic Boot Performance Table guarded by a TPL mutex.
///
/// The table is the only mutable state owned by the service. It is created at compile time using the core's TPL-aware
/// mutex, so the service needs no boot services instance to manage it.
static PERFORMANCE_TABLE: TplMutex<FBPT> = TplMutex::new(efi::TPL_NOTIFY, FBPT::new(), "PerformanceTableLock");

/// Performance measurement service owned by the DXE Core.
///
/// This is a stateless handle; all state lives in module statics. It is registered as a
/// [`Service<dyn PerformanceMeasurement>`] for components and used directly by core internals.
#[derive(IntoService)]
#[service(dyn PerformanceMeasurement)]
pub(crate) struct CorePerformance;

impl CorePerformance {
    /// Initializes the core performance service with the platform timer frequency.
    ///
    /// This is intentionally not part of the [`PerformanceMeasurement`] trait: the DXE Core owns its timer and
    /// initializes this itself rather than exposing initialization to service consumers. A frequency of `0` selects
    /// architecture-specific auto-detection.
    pub(crate) fn init(frequency: u64) {
        PERF_FREQUENCY.store(frequency, Ordering::Relaxed);
    }
}

impl PerformanceMeasurement for CorePerformance {
    fn measurement_mask(&self) -> u32 {
        PERF_MEASUREMENT_MASK.load(Ordering::Relaxed)
    }

    fn set_measurement_mask(&self, mask: u32) {
        PERF_MEASUREMENT_MASK.store(mask, Ordering::Relaxed);
    }

    fn set_load_image_count(&self, count: u32) {
        LOAD_IMAGE_COUNT.store(count, Ordering::Relaxed);
    }

    fn set_perf_records(&self, perf_records: PerformanceRecordBuffer) {
        PERFORMANCE_TABLE.lock().set_perf_records(perf_records);
    }

    fn create_measurement(
        &self,
        caller_identifier: CallerIdentifier,
        guid: Option<&efi::Guid>,
        string: Option<&str>,
        ticker: u64,
        address: usize,
        perf_id: u16,
        attribute: PerfAttribute,
    ) -> Result<(), Error> {
        create_measurement_impl(caller_identifier, guid, string, ticker, address, perf_id, attribute)
    }

    fn add_generic_record(&self, record: GenericPerformanceRecord<&[u8]>) -> Result<(), Error> {
        PERFORMANCE_TABLE.lock().add_record(record)
    }

    fn published_table_size(&self) -> Result<usize, Error> {
        Ok(PERFORMANCE_TABLE.lock().published_table_size())
    }

    fn publish_table(&self, buffer: &'static mut [u8]) -> Result<(), Error> {
        PERFORMANCE_TABLE.lock().publish_table(buffer).map(|_| ())
    }
}

/// Builds the appropriate performance record and adds it to the FBPT.
fn create_measurement_impl(
    caller_identifier: CallerIdentifier,
    guid: Option<&efi::Guid>,
    string: Option<&str>,
    ticker: u64,
    address: usize,
    perf_id: u16,
    attribute: PerfAttribute,
) -> Result<(), Error> {
    let timer = PerfTimer::with_frequency(PERF_FREQUENCY.load(Ordering::Relaxed));
    create_measurement_inner(
        caller_identifier,
        guid,
        string,
        ticker,
        address,
        perf_id,
        attribute,
        &PERFORMANCE_TABLE,
        &timer,
    )
}

/// Adds a record to the FBPT, returning an error if the table lock cannot be acquired.
///
/// The FBPT [`TplMutex`] must never be acquired re-entrantly. A re-entrant performance measurement
/// may occur when dropping a measurement's lock guard restores the TPL and dispatches a pending
/// event notification whose callback creates another measurement before the guard finishes releasing
/// the lock. To avoid panicking, this attempts lock acquisition and, on contention, drops the record and returns
/// [`EfiError::InvalidParameter`] so the caller can observe that the record was not added. This is similar
/// in behavior and return status to the edk2 implementation of [`create_performance_measurement`].
fn add_fbpt_record<F, T>(fbpt: &TplMutex<F>, record: T) -> Result<(), Error>
where
    F: FirmwareBasicBootPerfTable,
    T: PerformanceRecord,
{
    match fbpt.try_lock() {
        Some(mut table) => table.add_record(record),
        // Re-entrant measurement: See function docs.
        None => Err(EfiError::InvalidParameter.into()),
    }
}

/// Builds the appropriate performance record and adds it to the provided FBPT.
///
/// This is the generic core of [`create_measurement_impl`], split out so it can be unit-tested with a mock table.
#[allow(clippy::too_many_arguments)]
fn create_measurement_inner<F>(
    caller_identifier: CallerIdentifier,
    guid: Option<&efi::Guid>,
    string: Option<&str>,
    ticker: u64,
    address: usize,
    perf_id: u16,
    attribute: PerfAttribute,
    fbpt: &TplMutex<F>,
    timer: &impl ArchTimerFunctionality,
) -> Result<(), Error>
where
    F: FirmwareBasicBootPerfTable,
{
    let cpu_count = timer.cpu_count();
    let timestamp = match ticker {
        0 => (cpu_count as f64 / timer.perf_frequency() as f64 * 1_000_000_000_f64) as u64,
        1 => 0,
        ticker => (ticker as f64 / timer.perf_frequency() as f64 * 1_000_000_000_f64) as u64,
    };

    // If the `perf_id` is not a known one, we create a DynamicStringEventRecord.
    // In this case, `caller_id` can be either an image handle or a guid pointer.
    let Ok(known_perf_id) = KnownPerfId::try_from(perf_id) else {
        // PERF_ENTRY must have a matching start and end.
        // Unknown IDs cannot be matched, so we reject PERF_ENTRY for unknown IDs.
        if attribute == PerfAttribute::PerfEntry {
            return Err(EfiError::InvalidParameter.into());
        }

        let handle = caller_identifier.as_handle().ok_or(EfiError::InvalidParameter)?;

        // Mirroring EDK2 behavior, when the ID is unknown, we treat `caller_identifier` as a handle.
        let Ok(guid) = get_module_guid_from_handle(handle) else {
            log::error!("Performance: Could not find the guid for module handle: {handle:?}");
            return Err(EfiError::InvalidParameter.into());
        };
        let module_name = string.unwrap_or("unknown name");
        return match add_fbpt_record(fbpt, DynamicStringEventRecord::new(perf_id, 0, timestamp, guid, module_name)) {
            Ok(()) => Ok(()),
            Err(e) => Err(report_add_record_error(e)),
        };
    };

    let result = match known_perf_id {
        KnownPerfId::ModuleStart | KnownPerfId::ModuleEnd => {
            let module_handle = caller_identifier.as_handle().ok_or(EfiError::InvalidParameter)?;
            let Ok(guid) = get_module_guid_from_handle(module_handle) else {
                log::error!("Performance: Could not find the guid for module handle: {module_handle:?}");
                return Err(EfiError::InvalidParameter.into());
            };
            add_fbpt_record(fbpt, GuidEventRecord::new(perf_id, 0, timestamp, guid))
        }
        id @ KnownPerfId::ModuleLoadImageStart | id @ KnownPerfId::ModuleLoadImageEnd => {
            if id == KnownPerfId::ModuleLoadImageStart {
                LOAD_IMAGE_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            let module_handle = caller_identifier.as_handle().ok_or(EfiError::InvalidParameter)?;
            let Ok(guid) = get_module_guid_from_handle(module_handle) else {
                log::error!("Performance: Could not find the guid for module handle: {module_handle:?}");
                return Err(EfiError::InvalidParameter.into());
            };
            let load_image_count = LOAD_IMAGE_COUNT.load(Ordering::Relaxed) as u64;
            add_fbpt_record(fbpt, GuidQwordEventRecord::new(perf_id, 0, timestamp, guid, load_image_count))
        }
        KnownPerfId::ModuleDbStart
        | KnownPerfId::ModuleDbSupportStart
        | KnownPerfId::ModuleDbSupportEnd
        | KnownPerfId::ModuleDbStopStart
        | KnownPerfId::ModuleDbStopEnd => {
            let module_handle = caller_identifier.as_handle().ok_or(EfiError::InvalidParameter)?;
            let Ok(guid) = get_module_guid_from_handle(module_handle) else {
                log::error!("Performance: Could not find the guid for module handle: {module_handle:?}");
                return Err(EfiError::InvalidParameter.into());
            };
            add_fbpt_record(fbpt, GuidQwordEventRecord::new(perf_id, 0, timestamp, guid, address as u64))
        }
        KnownPerfId::ModuleDbEnd => {
            let module_handle = caller_identifier.as_handle().ok_or(EfiError::InvalidParameter)?;
            let Ok(guid) = get_module_guid_from_handle(module_handle) else {
                log::error!("Performance: Could not find the guid for module handle: {module_handle:?}");
                return Err(EfiError::InvalidParameter.into());
            };
            let module_name = "";
            add_fbpt_record(
                fbpt,
                GuidQwordStringEventRecord::new(perf_id, 0, timestamp, guid, address as u64, module_name),
            )
        }
        KnownPerfId::PerfEventSignalStart
        | KnownPerfId::PerfEventSignalEnd
        | KnownPerfId::PerfCallbackStart
        | KnownPerfId::PerfCallbackEnd => {
            let (Some(function_string), Some(guid)) = (string.as_ref(), guid) else {
                return Err(EfiError::InvalidParameter.into());
            };
            let module_guid = caller_identifier.as_guid().ok_or(EfiError::InvalidParameter)?;
            add_fbpt_record(
                fbpt,
                DualGuidStringEventRecord::new(
                    perf_id,
                    0,
                    timestamp,
                    (*module_guid).into(),
                    (*guid).into(),
                    function_string,
                ),
            )
        }
        KnownPerfId::PerfFunctionStart
        | KnownPerfId::PerfFunctionEnd
        | KnownPerfId::PerfInModuleStart
        | KnownPerfId::PerfInModuleEnd
        | KnownPerfId::PerfCrossModuleStart
        | KnownPerfId::PerfCrossModuleEnd
        | KnownPerfId::PerfEvent => {
            let module_guid = caller_identifier.as_guid().ok_or(EfiError::InvalidParameter)?;
            let string = string.unwrap_or("unknown name");
            add_fbpt_record(fbpt, DynamicStringEventRecord::new(perf_id, 0, timestamp, (*module_guid).into(), string))
        }
    };

    result.map_err(report_add_record_error)
}

/// Logs and forwards an error encountered while adding a record to the FBPT.
fn report_add_record_error(error: Error) -> Error {
    match error {
        Error::OutOfResources => {
            static HAS_BEEN_LOGGED: AtomicBool = AtomicBool::new(false);
            if HAS_BEEN_LOGGED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                log::info!("Performance: FBPT is full, can't add more performance records!");
            }
            Error::OutOfResources
        }
        e => {
            log::error!("Performance: Something went wrong in create_measurement. status_code: {e:?}");
            e
        }
    }
}

/// Resolves the firmware file GUID for the module backing the given handle.
fn get_module_guid_from_handle(handle: efi::Handle) -> Result<BinaryGuid, efi::Status> {
    let mut guid = patina::guids::ZERO;

    let loaded_image_ptr = 'find_loaded_image_protocol: {
        if let Ok(interface) = PROTOCOL_DB.get_interface_for_handle(handle, efi::protocols::loaded_image::PROTOCOL_GUID)
        {
            break 'find_loaded_image_protocol Some(interface as *const efi::protocols::loaded_image::Protocol);
        }

        // Fall back to a driver binding protocol on the handle and use its loaded image.
        if let Ok(driver_binding) =
            PROTOCOL_DB.get_interface_for_handle(handle, efi::protocols::driver_binding::PROTOCOL_GUID)
        {
            // SAFETY: `get_interface_for_handle` returned a valid driver binding protocol interface for this handle.
            let image_handle =
                unsafe { (*(driver_binding as *const efi::protocols::driver_binding::Protocol)).image_handle };
            if let Ok(interface) =
                PROTOCOL_DB.get_interface_for_handle(image_handle, efi::protocols::loaded_image::PROTOCOL_GUID)
            {
                break 'find_loaded_image_protocol Some(interface as *const efi::protocols::loaded_image::Protocol);
            }
        }
        None
    };

    if let Some(loaded_image_ptr) = loaded_image_ptr {
        // SAFETY: `loaded_image_ptr` is a valid pointer to a `loaded_image::Protocol` returned by the protocol database.
        let loaded_image = unsafe { &*loaded_image_ptr };
        // SAFETY: File path is a pointer from C that is valid and of type Device Path (efi).
        if let Some(file_path) = unsafe { loaded_image.file_path.as_ref() }
            && file_path.r#type == TYPE_MEDIA
            && file_path.sub_type == Media::SUBTYPE_PIWG_FIRMWARE_FILE
        {
            // The layout of MEDIA_FW_VOL_FILEPATH_DEVICE_PATH in memory is { Protocol (header) | Guid (file name) }.
            let node_len = u16::from_le_bytes(file_path.length);
            let expected_len =
                (mem::size_of::<efi::protocols::device_path::Protocol>() + mem::size_of::<efi::Guid>()) as u16;

            // Sanity check that the header matches the expected size.
            if node_len != expected_len {
                return Err(efi::Status::NOT_FOUND);
            }

            // SAFETY: To be honest there is no way to guarantee the memory read here is valid and owned by us,
            // but we have at least validated that the type gives a known layout and the layout of the device path
            // matches its claimed length.
            unsafe {
                let guid_ptr = (loaded_image.file_path as *const u8)
                    .add(mem::size_of::<efi::protocols::device_path::Protocol>())
                    as *const BinaryGuid;
                guid = ptr::read(guid_ptr);
            }
        };
    }

    Ok(guid)
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use patina::performance::table::MockFirmwareBasicBootPerfTable;
    use r_efi::efi;

    #[derive(IntoService)]
    #[service(dyn ArchTimerFunctionality)]
    struct MockTimer {}

    impl ArchTimerFunctionality for MockTimer {
        fn perf_frequency(&self) -> u64 {
            100
        }
        fn cpu_count(&self) -> u64 {
            200
        }
    }

    /// Exercises [`create_measurement_inner`] (and all the trait default `perf_*` helpers) against a mock table,
    /// verifying that each helper produces exactly one record.
    #[test]
    fn test_core_performance_create_measurement_all_records() {
        // Test-only implementation that routes `create_measurement` to `create_measurement_inner` with a mock table.
        struct TestPerf;
        static mut FBPT: Option<&TplMutex<MockFirmwareBasicBootPerfTable>> = None;

        impl PerformanceMeasurement for TestPerf {
            fn measurement_mask(&self) -> u32 {
                u32::MAX
            }
            fn set_measurement_mask(&self, _mask: u32) {}
            fn set_load_image_count(&self, _count: u32) {}
            fn set_perf_records(&self, _perf_records: PerformanceRecordBuffer) {}
            fn create_measurement(
                &self,
                caller_identifier: CallerIdentifier,
                guid: Option<&efi::Guid>,
                string: Option<&str>,
                ticker: u64,
                address: usize,
                perf_id: u16,
                attribute: PerfAttribute,
            ) -> Result<(), Error> {
                let timer = MockTimer {};
                create_measurement_inner(
                    caller_identifier,
                    guid,
                    string,
                    ticker,
                    address,
                    perf_id,
                    attribute,
                    // SAFETY: Test code - statics are initialized below before use on a single thread.
                    unsafe { FBPT.unwrap() },
                    &timer,
                )
            }
            fn add_generic_record(&self, record: GenericPerformanceRecord<&[u8]>) -> Result<(), Error> {
                // SAFETY: Test code - statics are initialized below before use on a single thread.
                unsafe { FBPT.unwrap() }.lock().add_record(record)
            }
            fn published_table_size(&self) -> Result<usize, Error> {
                Ok(0)
            }
            fn publish_table(&self, _buffer: &'static mut [u8]) -> Result<(), Error> {
                Ok(())
            }
        }

        const EXPECTED_NUMBER_OF_RECORD: usize = 21;
        let mut fbpt = MockFirmwareBasicBootPerfTable::new();
        fbpt.expect_add_record().times(EXPECTED_NUMBER_OF_RECORD).returning(|_| Ok(()));
        let fbpt = TplMutex::new(efi::TPL_NOTIFY, fbpt, "TestPerfTableLock");

        let module_handle = 1_usize as efi::Handle;
        let controller_handle = 2_usize as efi::Handle;
        let caller_id = efi::Guid::from_bytes(&[1; 16]);
        let trigger_guid = efi::Guid::from_bytes(&[2; 16]);
        let event_guid = efi::Guid::from_bytes(&[3; 16]);

        // SAFETY: Test code - initializing the static with the test reference before use.
        unsafe {
            FBPT = Some(&*ptr::addr_of!(fbpt));
        }

        let perf = TestPerf;
        perf.perf_image_start_begin(module_handle);
        perf.perf_image_start_end(module_handle);
        perf.perf_load_image_begin(module_handle);
        perf.perf_load_image_end(module_handle);
        perf.perf_driver_binding_support_begin(module_handle, controller_handle);
        perf.perf_driver_binding_support_end(module_handle, controller_handle);
        perf.perf_driver_binding_start_begin(module_handle, controller_handle);
        perf.perf_driver_binding_start_end(module_handle, controller_handle);
        perf.perf_driver_binding_stop_begin(module_handle, controller_handle);
        perf.perf_driver_binding_stop_end(module_handle, controller_handle);
        perf.perf_event("event_string", &caller_id);
        perf.perf_event_signal_begin(&event_guid, "fun_name", &caller_id);
        perf.perf_event_signal_end(&event_guid, "fun_name", &caller_id);
        perf.perf_callback_begin(&trigger_guid, "fun_name", &caller_id);
        perf.perf_callback_end(&trigger_guid, "fun_name", &caller_id);
        perf.perf_function_begin("fun_name", &caller_id);
        perf.perf_function_end("fun_name", &caller_id);
        perf.perf_in_module_begin("measurement_str", &caller_id);
        perf.perf_in_module_end("measurement_str", &caller_id);
        perf.perf_cross_module_begin("measurement_str", &caller_id);
        perf.perf_cross_module_end("measurement_str", &caller_id);
    }

    /// Verifies the validation paths of [`create_measurement_inner`] for unknown perf ids.
    #[test]
    fn test_core_performance_create_measurement_invalid_params() {
        let fbpt = TplMutex::new(efi::TPL_NOTIFY, MockFirmwareBasicBootPerfTable::new(), "TestPerfTableLock");
        let timer = MockTimer {};

        // A PerfEntry must have a known perf id.
        let unknown_perf_id = 0xFFFF;
        let result = create_measurement_inner(
            CallerIdentifier::Handle(0x1_usize as efi::Handle),
            None,
            Some("test"),
            0,
            0,
            unknown_perf_id,
            PerfAttribute::PerfEntry,
            &fbpt,
            &timer,
        );
        assert_eq!(result.unwrap_err(), Error::Efi(EfiError::InvalidParameter));

        // If the perf id is unknown, the caller identifier must be a handle.
        let result = create_measurement_inner(
            CallerIdentifier::Guid(efi::Guid::from_bytes(&[1; 16])),
            None,
            Some("test"),
            0,
            0,
            unknown_perf_id,
            PerfAttribute::PerfStartEntry,
            &fbpt,
            &timer,
        );
        assert_eq!(result.unwrap_err(), Error::Efi(EfiError::InvalidParameter));
    }
}
