use super::ServiceManager;
use lianli_devices::wireless::{
    DonglePresence, RecoveryAttempt, RecoveryLadder, RecoveryPlan, WirelessController,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tracing::{debug, info, warn};

struct RecoveryWorker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<(WirelessController, RecoveryAttempt)>,
}

#[derive(Default)]
pub(super) struct WirelessRecovery {
    ladder: RecoveryLadder,
    worker: Option<RecoveryWorker>,
    presence: DonglePresence,
    link_up: bool,
    ever_connected: bool,
    last_error: Option<String>,
}

impl WirelessRecovery {
    fn take_finished(&mut self) -> Option<thread::Result<(WirelessController, RecoveryAttempt)>> {
        if !self.worker.as_ref()?.handle.is_finished() {
            return None;
        }
        self.worker.take().map(|worker| worker.handle.join())
    }

    pub(super) fn cancel(&self) {
        if let Some(worker) = &self.worker {
            worker.stop.store(true, Ordering::Release);
        }
    }

    pub(super) fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        worker.stop.store(true, Ordering::Release);
        if worker.handle.join().is_err() {
            warn!("Wireless recovery worker panicked during shutdown");
        }
    }
}

impl ServiceManager {
    pub(super) fn connect_wireless_at_startup(&mut self) {
        let presence = DonglePresence::scan();
        self.wireless_recovery.presence = presence;
        if presence.tx.is_none() {
            self.report_missing_transmitter();
            return;
        }
        let result = self
            .wireless
            .connect()
            .and_then(|()| self.wireless.start_polling());
        self.finish_wireless_attempt(RecoveryAttempt {
            reset: None,
            result,
        });
    }

    pub(super) fn poll_wireless_recovery(&mut self) {
        let Some(finished) = self.wireless_recovery.take_finished() else {
            return;
        };
        // A companion reset re-enumerates the dongle; that must not count as hotplug.
        self.wireless_recovery.presence = DonglePresence::scan();
        let attempt = match finished {
            Ok((controller, attempt)) => {
                if attempt.result.is_ok() {
                    self.wireless = controller;
                }
                attempt
            }
            Err(_) => RecoveryAttempt {
                reset: None,
                result: Err(anyhow::anyhow!("wireless recovery worker panicked")),
            },
        };
        self.finish_wireless_attempt(attempt);
    }

    pub(super) fn supervise_wireless(&mut self) {
        self.poll_wireless_recovery();
        if self.wireless_recovery.worker.is_some() {
            return;
        }
        if self.wireless.is_connected() {
            if self.wireless_rx_init_pending {
                self.init_wireless_receiver();
            }
            return;
        }
        if std::mem::take(&mut self.wireless_recovery.link_up) {
            warn!("Wireless link lost; releasing the dongles and starting recovery");
            self.release_wireless();
            self.ipc.state.lock().state_health.wireless_unavailable(
                "The wireless link was lost. The daemon is reconnecting the dongles.",
            );
        }

        let now = Instant::now();
        let presence = DonglePresence::scan();
        if presence != self.wireless_recovery.presence {
            debug!(?presence, "Wireless dongle presence changed");
            self.wireless_recovery.presence = presence;
            self.wireless_recovery.ladder.hotplug(now);
        }
        let recoverable = presence.tx.is_some()
            || (presence.rx.is_some()
                && (self.wireless_recovery.ever_connected || self.wireless_configured()));
        if !recoverable {
            self.report_missing_transmitter();
            return;
        }
        if let Some(plan) = self.wireless_recovery.ladder.next_attempt(now) {
            self.start_wireless_recovery(plan);
        }
    }

    fn release_wireless(&mut self) {
        if let Some(rgb) = &self.controllers.rgb {
            rgb.lock().set_wireless(None);
        }
        self.wireless.stop();
    }

    fn start_wireless_recovery(&mut self, plan: RecoveryPlan) {
        self.release_wireless();
        let mut controller = self.wireless.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        debug!(?plan, "Starting wireless recovery attempt");
        let spawned = thread::Builder::new()
            .name("wireless-recovery".into())
            .spawn(move || {
                let attempt = controller.recover(plan, &worker_stop);
                (controller, attempt)
            });
        match spawned {
            Ok(handle) => self.wireless_recovery.worker = Some(RecoveryWorker { stop, handle }),
            Err(error) => self.finish_wireless_attempt(RecoveryAttempt {
                reset: None,
                result: Err(anyhow::Error::new(error).context("spawning wireless recovery")),
            }),
        }
    }

    fn finish_wireless_attempt(&mut self, attempt: RecoveryAttempt) {
        self.wireless_recovery
            .ladder
            .record(&attempt, Instant::now());
        if let Err(error) = attempt.result {
            let error = format!("{error:#}");
            self.ipc
                .state
                .lock()
                .state_health
                .wireless_unavailable(&error);
            if self.wireless_recovery.last_error.as_ref() != Some(&error) {
                warn!(%error, "Wireless reconnect failed; wireless fans and pumps keep their last speed until it recovers");
                self.wireless_recovery.last_error = Some(error);
            } else {
                debug!(%error, "Wireless reconnect failed again");
            }
            return;
        }
        if self.wireless_recovery.last_error.take().is_some() {
            info!(reset = ?attempt.reset, "Wireless link recovered");
        }
        self.wireless_recovery.link_up = true;
        self.wireless_recovery.ever_connected = true;
        self.init_wireless_receiver();
        info!("Wireless links active");
        if self.controllers.fan.is_some() || self.controllers.aio.is_some() {
            self.start_fan_control();
            self.start_aio_control();
        }
        self.rebuild_rgb_controller();
    }

    fn wireless_configured(&self) -> bool {
        self.configured_wireless_device_ids()
            .iter()
            .any(|id| id.starts_with("wireless:"))
    }

    fn report_missing_transmitter(&mut self) {
        let configured = self.wireless_configured();
        let mut state = self.ipc.state.lock();
        if configured {
            state.state_health.wireless_unavailable(
                "Saved settings reference wireless devices, but no transmitter was detected.",
            );
        } else if self.wireless_recovery.ever_connected {
            state.state_health.wireless_unavailable(
                "The wireless transmitter disconnected and has not returned.",
            );
        } else {
            state.state_health.wireless_ready();
        }
    }
}
