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
mod search_index;
pub mod song_info;
pub const SUPPORTED_SHA256: &str =
    "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d";

#[cfg(windows)]
pub(crate) fn attach(
    image: crate::host::windows::ModuleImage,
    controls: &crate::config::Controls,
) -> anyhow::Result<Box<dyn crate::game::GameAdapter>> {
    native::configure_controls(controls.clone());
    native::install(image)?;
    Ok(Box::new(Adapter))
}

#[cfg(windows)]
struct Adapter;

#[cfg(windows)]
impl crate::game::GameAdapter for Adapter {
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
        let mut m = native::MAILBOX.lock().unwrap();
        let toggle_menu = m
            .menu_event
            .take()
            .is_some_and(|e| m.menu_side == Some(e.side) && m.snapshot.epoch == e.epoch);
        crate::game::GameUpdate {
            toggle_menu,
            navigation: m.navigation.drain(..).collect(),
            snapshot: m.snapshot,
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
