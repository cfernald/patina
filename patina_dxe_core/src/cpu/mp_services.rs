//! MP Services Protocol installation.
//!
//! Hosts the [`MpServicesProtocolInstaller`] component, which brings Application
//! Processors online via the CPU crate's MP subsystem, parks them at
//! ExitBootServices, and installs the EFI_MP_SERVICES_PROTOCOL. The rusty backing
//! state lives in the `services` submodule, the ABI wrapper in `protocol`, and
//! the `MpInformation2` HOB parser in `hob`.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::{boxed::Box, vec::Vec};

use patina::{
    component::{
        component,
        hob::Hob,
        service::{Service, memory::MemoryManager, perf_timer::ArchTimerFunctionality},
    },
    error::EfiError,
    standard::efi::{self, protocols::mp_services},
    uefi::{
        boot_services::{BootServices, StandardBootServices, tpl::Tpl},
        event::{CACHE_ATTRIBUTE_CHANGE_EVENT_GROUP_GUID, EventTimerType, EventType},
    },
};

mod hob;
mod notification;
mod protocol;
mod services;

use hob::{MpHandOff, MpHandOffConfig, MpInformation2};
use patina_internal_cpu::mp::{MpDispatcher, MpHandOffInfo, MpSupport, ProcessorHandOff};
use protocol::MpProtocolWrapper;
use services::MpServices;

/// Period of the non-blocking notification poll timer, in 100 ns units (100 us).
const NOTIFICATION_POLL_PERIOD: u64 = 1_000;

/// Event group signaled when cache (MTRR) attributes change, driving AP resync.
static CACHE_ATTRIBUTE_CHANGE_GUID: efi::Guid = CACHE_ATTRIBUTE_CHANGE_EVENT_GROUP_GUID.into_inner();

fn flatten_indexed<T: Copy>(mut chunks: Vec<(usize, &[T])>, name: &str) -> Vec<T> {
    chunks.sort_unstable_by_key(|(index, _)| *index);
    let mut values = Vec::new();
    for (index, chunk) in chunks {
        if index != values.len() {
            log::error!("Invalid {name} HOB processor index {index}; expected {}", values.len());
            return Vec::new();
        }
        values.extend_from_slice(chunk);
    }
    values
}

/// This component installs the MP Services Protocol.
#[derive(Default)]
pub(crate) struct MpServicesProtocolInstaller;

#[component]
impl MpServicesProtocolInstaller {
    fn entry_point(
        self,
        bs: StandardBootServices,
        mm: Service<dyn MemoryManager>,
        timer: Service<dyn ArchTimerFunctionality>,
        mp_info: Option<Hob<MpInformation2>>,
        mp_handoff: Option<Hob<MpHandOff>>,
        mp_handoff_config: Option<Hob<MpHandOffConfig>>,
    ) -> Result<(), EfiError> {
        // Gather whatever PEI-to-DXE handoff the platform provided.
        let processors: Vec<ProcessorHandOff> = mp_handoff
            .as_ref()
            .map(|hob| {
                flatten_indexed(
                    hob.iter().map(|inst| (inst.processor_index(), inst.processors())).collect(),
                    "MP_HAND_OFF",
                )
            })
            .unwrap_or_default();

        // Aggregate the processor information from every HOB instance.
        let processor_infos: Vec<mp_services::ProcessorInformation> = mp_info
            .as_ref()
            .map(|hob| {
                flatten_indexed(
                    hob.iter().map(|inst| (inst.processor_index(), inst.processors())).collect(),
                    "MP_INFORMATION2",
                )
            })
            .unwrap_or_default();

        let handoff = mp_handoff_config
            .as_ref()
            .and_then(|cfg| cfg.iter().next())
            .map(|config| MpHandOffInfo {
                wait_loop_execution_mode: config.wait_loop_execution_mode,
                startup_signal_value: config.startup_signal_value,
                processors: &processors,
            })
            .filter(|handoff| {
                if !processor_infos.is_empty() && processor_infos.len() != handoff.processors.len() {
                    log::error!(
                        "MP handoff describes {} processors but MP_INFORMATION2 describes {}; continuing with the BSP only",
                        handoff.processors.len(),
                        processor_infos.len()
                    );
                    return false;
                }
                true
            });

        let mp = MpSupport::initialize(*mm, *timer, handoff)
            .inspect_err(|e| log::error!("Failed to initialize MP architecture support: {e:?}"))?;

        let services: &'static MpServices = Box::leak(Box::new(MpServices::new(mp, processor_infos, *timer)));

        // Baseline: bring the APs' caching state in line with the BSP's before anything
        // can dispatch to them.
        services.synchronize();

        // Create park event for EBS so that APs are parked before the OS takes over.
        bs.create_event_ex(
            EventType::NOTIFY_SIGNAL,
            Tpl::CALLBACK,
            Some(park_aps_on_exit),
            services,
            &efi::EVENT_GROUP_EXIT_BOOT_SERVICES,
        )
        .inspect_err(|e| log::error!("Failed to register MP AP-park event: {e:?}"))?;

        bs.create_event_ex(
            EventType::NOTIFY_SIGNAL,
            Tpl::CALLBACK,
            Some(mark_mp_ready_to_boot),
            services,
            &efi::EVENT_GROUP_READY_TO_BOOT,
        )
        .inspect_err(|e| log::error!("Failed to register MP ReadyToBoot event: {e:?}"))?;

        // Create cache-attribute-change event AP synchronization.
        bs.create_event_ex(
            EventType::NOTIFY_SIGNAL,
            Tpl::CALLBACK,
            Some(sync_mtrrs_on_cache_change),
            services,
            &CACHE_ATTRIBUTE_CHANGE_GUID,
        )
        .inspect_err(|e| log::error!("Failed to register MP MTRR sync event: {e:?}"))?;

        // Create a periodic timer to poll for non-blocking dispatches.
        let timer_event = bs
            .create_event(
                EventType::TIMER | EventType::NOTIFY_SIGNAL,
                Tpl::NOTIFY,
                Some(poll_mp_notifications),
                services,
            )
            .inspect_err(|e| log::error!("Failed to create MP notification timer event: {e:?}"))?;

        bs.set_timer(timer_event, EventTimerType::Periodic, NOTIFICATION_POLL_PERIOD)
            .inspect_err(|e| log::error!("Failed to arm MP notification timer: {e:?}"))?;

        // Install the protocol wrapper
        let protocol = Box::leak(Box::new(MpProtocolWrapper::new(services, bs.clone())));
        bs.install_protocol_interface(None, protocol)
            .inspect_err(|_| log::error!("Failed to install MP_SERVICES_PROTOCOL"))?;

        Ok(())
    }
}

/// Periodic timer callback used to process non-blocking dispatches.
extern "efiapi" fn poll_mp_notifications(_event: efi::Event, services: &'static MpServices) {
    services.poll_notifications();
}

extern "efiapi" fn mark_mp_ready_to_boot(_event: efi::Event, services: &'static MpServices) {
    services.mark_ready_to_boot();
}
/// ExitBootServices callback to park all APs.
extern "efiapi" fn park_aps_on_exit(_event: efi::Event, services: &'static MpServices) {
    services.park();
    log::info!("MP Services: APs parked for ExitBootServices");
}

/// Cache-attribute-change callback that re-synchronizes AP MTRRs to the BSP.
extern "efiapi" fn sync_mtrrs_on_cache_change(_event: efi::Event, services: &'static MpServices) {
    services.synchronize();
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_mp_services_flattens_hob_chunks_by_processor_index() {
        let first = [10u32, 11];
        let second = [12u32, 13];

        assert_eq!(flatten_indexed(vec![(2, &second), (0, &first)], "test"), [10, 11, 12, 13]);
    }

    #[test]
    fn test_mp_services_rejects_noncontiguous_hob_chunks() {
        let first = [10u32];
        let second = [12u32];

        assert!(flatten_indexed(vec![(0, &first), (2, &second)], "test").is_empty());
    }
}
