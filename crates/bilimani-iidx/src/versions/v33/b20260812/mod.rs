//! The verified IIDX 33 binary profile. No offsets escape this module.
pub mod catalog;
pub mod density;
#[cfg(windows)]
mod input_hook;
pub mod lane_order;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod native_density;
pub mod playback;
#[cfg(windows)]
pub(crate) mod search_index;
pub mod song_info;
pub mod user;
const PARTICIPATION_RVA: usize = 0xacd79b0;
pub const SUPPORTED_SHA256: &str =
    "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d";

/// Scene callbacks stop on logout, so their last SP/DP mode can outlive the players.
/// Only an explicit empty participation sample outside selection/play clears it.
#[cfg(any(windows, test))]
fn session_snapshot(
    snapshot: crate::game::Snapshot,
    participants: Option<&[u8]>,
) -> crate::game::Snapshot {
    match (snapshot.phase, participants) {
        (crate::game::Phase::Other, Some([0, 0, 0, 0, 0, 0, 0, 0])) => crate::game::Snapshot {
            mode: None,
            can_open_menu: false,
            ..snapshot
        },
        _ => snapshot,
    }
}

#[cfg(windows)]
pub(crate) fn attach(
    image: crate::host::windows::ModuleImage,
    controls: &crate::config::Controls,
) -> anyhow::Result<Box<dyn crate::game::GameAdapter>> {
    native::configure_controls(controls.clone());
    let base = image.base;
    let user_enabled = user::GUARDS.iter().all(|&(rva, expected)| {
        native::read_memory(base + rva, 16).is_ok_and(|bytes| hex::encode(bytes) == expected)
    });
    native::install(image)?;
    Ok(Box::new(Adapter { base, user_enabled }))
}

#[cfg(windows)]
struct Adapter {
    base: usize,
    user_enabled: bool,
}

#[cfg(windows)]
impl crate::game::GameAdapter for Adapter {
    fn build(&self) -> crate::versions::Build {
        crate::versions::Build::V33_20260812
    }
    fn set_menu_open(&self, open: bool) {
        native::MENU_OPEN.store(open, std::sync::atomic::Ordering::Release);
    }
    fn cancel_selection(&self) {
        let mut m = native::MAILBOX.lock().unwrap();
        m.command = None;
        m.ack = None;
    }
    fn startup_messages(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "game",
                "Native hooks installed for the verified IIDX 33 build; waiting for song select."
                    .into(),
            ),
            ("input", native::input_chain().into()),
            ("lane", native::lane_order_status().into()),
            ("playback", native::playback_status().into()),
            (
                "user",
                if self.user_enabled {
                    "Player sampling enabled"
                } else {
                    "Player sampling disabled: native guards changed"
                }
                .into(),
            ),
        ]
    }
    fn catalog(
        &self,
        override_path: &std::path::Path,
    ) -> anyhow::Result<Option<Vec<crate::game::Song>>> {
        if !override_path.as_os_str().is_empty() {
            return catalog::parse_database(&std::fs::read(override_path)?)
                .map(Some)
                .map_err(Into::into);
        }
        native::LIVE_DATABASE
            .get()
            .map(|bytes| catalog::parse_database(bytes))
            .transpose()
            .map_err(Into::into)
    }
    fn take_search_index(&self) -> Option<Vec<(u32, String)>> {
        native::SEARCH_INDEX.lock().unwrap().take()
    }
    fn poll(&self) -> crate::game::GameUpdate {
        let user = if self.user_enabled {
            user::sample(
                |address, size| {
                    native::read_memory(address, size).map_err(|_| user::SampleError::Unreadable)
                },
                self.base,
            )
        } else {
            Default::default()
        };
        let mut m = native::MAILBOX.lock().unwrap();
        // Read the already-verified participation flags even after scene hooks stop.
        // Profile availability (including guest play) is not a session-end signal.
        let participants = native::read_memory(self.base + PARTICIPATION_RVA, 8).ok();
        let toggle_menu = m
            .menu_event
            .take()
            .is_some_and(|e| m.menu_side == Some(e.side) && m.snapshot.epoch == e.epoch);
        crate::game::GameUpdate {
            user,
            toggle_menu,
            navigation: m.navigation.drain(..).collect(),
            snapshot: session_snapshot(m.snapshot, participants.as_deref()),
            now_playing: m.now_playing.clone(),
            plays: m.plays,
            selection_result: m.ack.take(),
        }
    }

    fn submit(&self, selection: crate::game::Selection) {
        let mut m = native::MAILBOX.lock().unwrap();
        if !matches!(selection.mode, crate::iidx::SP | crate::iidx::DP)
            || selection
                .chart
                .is_some_and(|c| c.mode != selection.mode || crate::iidx::difficulty(c).is_none())
        {
            m.ack = Some(crate::game::SelectionResult {
                token: selection.token,
                result: Some(Err(crate::game::SelectionError::InvalidChart)),
            });
        } else {
            m.command = Some(selection);
        }
    }
    fn diagnostics(&self) -> String {
        native::diagnostics()
    }
    fn disabled(&self) -> bool {
        native::DISABLED.load(std::sync::atomic::Ordering::Acquire)
    }
    fn stop(&self) {
        self.set_menu_open(false);
        native::DISABLED.store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::session_snapshot;
    use crate::{
        game::{Phase, Snapshot},
        iidx,
    };

    #[test]
    fn logout_clears_stale_mode_after_scene_callbacks_stop() {
        for mode in [iidx::SP, iidx::DP] {
            let previous = Snapshot {
                phase: Phase::Other,
                mode: Some(mode),
                epoch: 7,
                can_open_menu: true,
            };
            let current = session_snapshot(previous, Some(&[0; 8]));
            assert_eq!(current.mode, None);
            assert_eq!(current.phase, Phase::Other);
            assert_eq!(current.epoch, previous.epoch);
            assert!(!current.can_open_menu);
            assert_eq!(
                previous.mode,
                Some(mode),
                "sampling does not overwrite game-thread state"
            );
        }
    }

    #[test]
    fn participation_preserves_mode_during_results_and_guest_play() {
        let previous = Snapshot {
            phase: Phase::Other,
            mode: Some(iidx::SP),
            ..Default::default()
        };
        for participants in [
            [1, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 1, 0, 0, 0],
            [1, 0, 0, 0, 1, 0, 0, 0],
        ] {
            assert_eq!(
                session_snapshot(previous, Some(&participants)).mode,
                previous.mode
            );
        }
    }

    #[test]
    fn failed_reads_and_active_scenes_cannot_be_mistaken_for_logout() {
        let previous = Snapshot {
            mode: Some(iidx::DP),
            ..Default::default()
        };
        for participants in [None, Some([0; 4].as_slice()), Some([0; 9].as_slice())] {
            assert_eq!(session_snapshot(previous, participants).mode, previous.mode);
        }
        for phase in [Phase::Select, Phase::Playing] {
            assert_eq!(
                session_snapshot(Snapshot { phase, ..previous }, Some(&[0; 8])).mode,
                previous.mode
            );
        }
    }
}
