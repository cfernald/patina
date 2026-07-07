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

use alloc::vec::Vec;

use patina::{
    BinaryGuid,
    component::{
        hob::FromHob,
        service::{IntoService, perf_timer::ArchTimerFunctionality, performance::PerformanceMeasurement},
    },
    error::EfiError,
    performance::{
        Measurement,
        config::PerformanceConfig,
        error::Error,
        measurement::CallerIdentifier,
        record::{
            GenericPerformanceRecord, PerformanceRecord, PerformanceRecordBuffer,
            extended::{
                DualGuidStringEventRecord, DynamicStringEventRecord, GuidEventRecord, GuidQwordEventRecord,
                GuidQwordStringEventRecord,
            },
            hob::{HobPerformanceData, merge_hob_performance_buffer},
            known::KnownPerfId,
        },
        table::{FBPT, FirmwareBasicBootPerfTable},
    },
    pi::hob::{Hob as PiHob, HobList},
    uefi_protocol::performance_measurement::PerfAttribute,
};

use crate::{cpu::PerfTimer, protocols::PROTOCOL_DB, tpl_mutex::TplMutex};

use r_efi::{
    efi,
    protocols::device_path::{Media, TYPE_MEDIA},
};

static LOAD_IMAGE_COUNT: AtomicU32 = AtomicU32::new(0);
static PERF_MEASUREMENT_MASK: AtomicU32 = AtomicU32::new(0);
static PERF_FREQUENCY: AtomicU64 = AtomicU64::new(0);
static PERFORMANCE_TABLE: TplMutex<FBPT> = TplMutex::new(efi::TPL_NOTIFY, FBPT::new(), "PerformanceTableLock");

macro_rules! gate_measurement {
    ($measurement:expr) => {
        if PERF_MEASUREMENT_MASK.load(Ordering::Relaxed) & $measurement as u32 == 0 {
            return;
        }
    };
}

/// Performance measurement service owned by the DXE Core.
///
/// This is a stateless handle; all state lives in module statics. It is registered as a
/// [`Service<dyn PerformanceMeasurement>`] for components and used directly by core internals.
#[derive(IntoService)]
#[service(dyn PerformanceMeasurement)]
pub(crate) struct CorePerformance;

impl CorePerformance {
    /// Initializes the core performance service.
    pub(crate) fn init(frequency: u64, config: PerformanceConfig, hob_records: Option<(u32, PerformanceRecordBuffer)>) {
        PERF_FREQUENCY.store(frequency, Ordering::Relaxed);

        if config.enabled == PerformanceConfig::DISABLED {
            return;
        }

        PERF_MEASUREMENT_MASK.store(config.enabled_measurements, Ordering::Relaxed);

        if let Some((load_image_count, perf_records)) = hob_records {
            LOAD_IMAGE_COUNT.store(load_image_count, Ordering::Relaxed);
            PERFORMANCE_TABLE.lock().set_perf_records(perf_records);
        }

        // This PEI end and DXE begin are later into the DXE phase than ideal. However, this cannot be improved without
        // integrating performance earlier into the core.
        let dxe_core_guid = patina::guids::DXE_CORE.into_inner();
        CorePerformance.perf_cross_module_end("PEI", &dxe_core_guid);
        CorePerformance.perf_cross_module_begin("DXE", &dxe_core_guid);
    }

    pub(crate) fn enabled() -> bool {
        PERF_MEASUREMENT_MASK.load(Ordering::Relaxed) != 0
    }

    /// Begins performance measurement of start image in core.
    pub(crate) fn perf_image_start_begin(&self, module_handle: efi::Handle) {
        gate_measurement!(Measurement::StartImage);
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
    pub(crate) fn perf_image_start_end(&self, image_handle: efi::Handle) {
        gate_measurement!(Measurement::StartImage);
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
    pub(crate) fn perf_load_image_begin(&self, module_handle: efi::Handle) {
        gate_measurement!(Measurement::LoadImage);
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
    pub(crate) fn perf_load_image_end(&self, module_handle: efi::Handle) {
        gate_measurement!(Measurement::LoadImage);
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
    pub(crate) fn perf_driver_binding_support_begin(
        &self,
        driver_binding_handle: efi::Handle,
        controller_handle: efi::Handle,
    ) {
        gate_measurement!(Measurement::DriverBindingSupport);
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
    pub(crate) fn perf_driver_binding_support_end(
        &self,
        driver_binding_handle: efi::Handle,
        controller_handle: efi::Handle,
    ) {
        gate_measurement!(Measurement::DriverBindingSupport);
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
    pub(crate) fn perf_driver_binding_start_begin(
        &self,
        driver_binding_handle: efi::Handle,
        controller_handle: efi::Handle,
    ) {
        gate_measurement!(Measurement::DriverBindingStart);
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
    pub(crate) fn perf_driver_binding_start_end(
        &self,
        driver_binding_handle: efi::Handle,
        controller_handle: efi::Handle,
    ) {
        gate_measurement!(Measurement::DriverBindingStart);
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
    pub(crate) fn perf_driver_binding_stop_begin(&self, module_handle: efi::Handle, controller_handle: efi::Handle) {
        gate_measurement!(Measurement::DriverBindingStop);
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
    pub(crate) fn perf_driver_binding_stop_end(&self, module_handle: efi::Handle, controller_handle: efi::Handle) {
        gate_measurement!(Measurement::DriverBindingStop);
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
}

/// Reads the [`PerformanceConfig`] from the HOB list, returning a disabled default when no such HOB is present.
pub(crate) fn read_performance_config(hob_list: &HobList) -> Option<PerformanceConfig> {
    for hob in hob_list.iter() {
        if let PiHob::GuidHob(guid, data) = hob
            && guid.name == PerformanceConfig::HOB_GUID
        {
            return Some(PerformanceConfig::parse(data));
        }
    }
    None
}

/// Reads and merges any performance records carried over from a previous phase via HOBs.
///
/// Returns `None` when no performance record HOBs are present.
pub(crate) fn read_hob_performance_records(hob_list: &HobList) -> Option<(u32, PerformanceRecordBuffer)> {
    let perf_hobs: Vec<HobPerformanceData> = hob_list
        .iter()
        .filter_map(|hob| match hob {
            PiHob::GuidHob(guid, data) if guid.name == HobPerformanceData::HOB_GUID => {
                Some(HobPerformanceData::parse(data))
            }
            _ => None,
        })
        .collect();

    if perf_hobs.is_empty() {
        return None;
    }

    merge_hob_performance_buffer(perf_hobs.iter())
        .inspect_err(|e| log::error!("Performance: failed to merge HOB performance records: {e:?}"))
        .ok()
}

impl PerformanceMeasurement for CorePerformance {
    fn set_measurement_mask(&self, mask: u32) {
        PERF_MEASUREMENT_MASK.store(mask, Ordering::Relaxed);
    }

    fn set_load_image_count(&self, count: u32) {
        LOAD_IMAGE_COUNT.store(count, Ordering::Relaxed);
    }

    fn set_perf_records(&self, perf_records: PerformanceRecordBuffer) {
        match PERFORMANCE_TABLE.try_lock() {
            Some(mut table) => table.set_perf_records(perf_records),
            None => log::error!("Performance: failed to set performance records"),
        }
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
        add_fbpt_record(&PERFORMANCE_TABLE, record)
    }

    fn published_table_size(&self) -> Result<usize, Error> {
        match PERFORMANCE_TABLE.try_lock() {
            Some(table) => Ok(table.published_table_size()),
            None => Err(EfiError::InvalidParameter.into()),
        }
    }

    fn publish_table(&self, buffer: &'static mut [u8]) -> Result<(), Error> {
        match PERFORMANCE_TABLE.try_lock() {
            Some(mut table) => table.publish_table(buffer).map(|_| ()),
            None => Err(EfiError::InvalidParameter.into()),
        }
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
    use crate::test_support::with_global_lock;
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

    /// Exercises every record-building arm of [`create_measurement_inner`] against a mock table, verifying each
    /// known performance id produces exactly one record.
    #[test]
    fn test_core_performance_create_measurement_all_records() {
        with_global_lock(|| {
            const EXPECTED_NUMBER_OF_RECORD: usize = 21;
            let mut fbpt = MockFirmwareBasicBootPerfTable::new();
            fbpt.expect_add_record().times(EXPECTED_NUMBER_OF_RECORD).returning(|_| Ok(()));
            let fbpt = TplMutex::new(efi::TPL_NOTIFY, fbpt, "TestPerfTableLock");
            let timer = MockTimer {};

            let module_handle = 1_usize as efi::Handle;
            let caller_guid = efi::Guid::from_bytes(&[1; 16]);
            let event_guid = efi::Guid::from_bytes(&[2; 16]);

            macro_rules! measure {
                ($caller:expr, $guid:expr, $string:expr, $perf_id:ident) => {
                    create_measurement_inner(
                        $caller,
                        $guid,
                        $string,
                        0,
                        0,
                        KnownPerfId::$perf_id.as_u16(),
                        PerfAttribute::PerfEntry,
                        &fbpt,
                        &timer,
                    )
                    .unwrap();
                };
            }

            // Handle-based records (GUID resolved from the handle, which yields ZERO here).
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleStart);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleEnd);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleLoadImageStart);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleLoadImageEnd);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbStart);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbSupportStart);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbSupportEnd);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbStopStart);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbStopEnd);
            measure!(CallerIdentifier::Handle(module_handle), None, None, ModuleDbEnd);

            // Dual-guid + string records (require a caller GUID, a trigger GUID, and a string).
            measure!(CallerIdentifier::Guid(caller_guid), Some(&event_guid), Some("fun_name"), PerfEventSignalStart);
            measure!(CallerIdentifier::Guid(caller_guid), Some(&event_guid), Some("fun_name"), PerfEventSignalEnd);
            measure!(CallerIdentifier::Guid(caller_guid), Some(&event_guid), Some("fun_name"), PerfCallbackStart);
            measure!(CallerIdentifier::Guid(caller_guid), Some(&event_guid), Some("fun_name"), PerfCallbackEnd);

            // Dynamic-string records (require a caller GUID and a string).
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfFunctionStart);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfFunctionEnd);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfInModuleStart);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfInModuleEnd);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfCrossModuleStart);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfCrossModuleEnd);
            measure!(CallerIdentifier::Guid(caller_guid), None, Some("measurement_str"), PerfEvent);
        })
        .unwrap();
    }

    /// Verifies the validation paths of [`create_measurement_inner`] for unknown perf ids.
    #[test]
    fn test_core_performance_create_measurement_invalid_params() {
        with_global_lock(|| {
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
        })
        .unwrap();
    }

    /// Verifies that [`read_performance_config`] reads the config HOB when present and falls back to a disabled
    /// default when absent.
    #[test]
    fn test_core_performance_read_performance_config() {
        use patina::pi::hob::{GUID_EXTENSION, GuidHob, header};

        // Absent: returns a disabled default.
        let empty = HobList::new();
        let config = read_performance_config(&empty);
        assert!(config.is_none());

        // Present: enabled with a measurement mask of 0x9 (packed u8 + u32, little-endian).
        let bytes: [u8; 5] = [PerformanceConfig::ENABLED, 0x09, 0x00, 0x00, 0x00];
        let guid_hob = GuidHob {
            header: header::Hob { r#type: GUID_EXTENSION, length: 0, reserved: 0 },
            name: PerformanceConfig::HOB_GUID,
        };
        let mut hob_list = HobList::new();
        hob_list.push(PiHob::GuidHob(&guid_hob, &bytes));

        let config = read_performance_config(&hob_list).expect("config present");
        assert_eq!(config.enabled, PerformanceConfig::ENABLED);
        assert_eq!({ config.enabled_measurements }, 0x9);
    }

    /// Verifies that [`read_hob_performance_records`] merges carried-over performance HOBs and returns `None` when
    /// none are present.
    #[test]
    fn test_core_performance_read_hob_performance_records() {
        use patina::pi::hob::{GUID_EXTENSION, GuidHob, header};

        // Absent: returns None.
        let empty = HobList::new();
        assert!(read_hob_performance_records(&empty).is_none());

        // Present: a single HOB carrying a load-image count and no records.
        const LOAD_IMAGE_COUNT: u32 = 7;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes()); // size_of_all_entries
        bytes.extend_from_slice(&LOAD_IMAGE_COUNT.to_le_bytes()); // load_image_count
        bytes.extend_from_slice(&0u32.to_le_bytes()); // hob_is_full
        let guid_hob = GuidHob {
            header: header::Hob { r#type: GUID_EXTENSION, length: 0, reserved: 0 },
            name: HobPerformanceData::HOB_GUID,
        };
        let mut hob_list = HobList::new();
        hob_list.push(PiHob::GuidHob(&guid_hob, &bytes));

        let (load_image_count, records) = read_hob_performance_records(&hob_list).expect("records present");
        assert_eq!(load_image_count, LOAD_IMAGE_COUNT);
        assert_eq!(records.iter().count(), 0);
    }

    /// Verifies that [`CorePerformance::init`] stores the timer frequency and honors the disable gate. The enabled
    /// path drives the global FBPT (and thus the global TPL) and is exercised at the integration level instead.
    #[test]
    fn test_core_performance_init_disabled_stores_frequency() {
        const FREQ: u64 = 987_654;
        let config = PerformanceConfig::new();
        assert_eq!(config.enabled, PerformanceConfig::DISABLED);

        CorePerformance::init(FREQ, config, None);

        assert_eq!(PERF_FREQUENCY.load(Ordering::Relaxed), FREQ);
    }
}
