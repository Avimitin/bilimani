//! The verified IIDX 33 binary profile. No offsets escape this module.
pub mod catalog;
#[cfg(windows)]
mod input_hook;
#[cfg(windows)]
mod native;
pub mod player;
#[cfg(windows)]
mod search_index;
pub mod song_info;
pub const SUPPORTED_SHA256: &str =
    "c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d";

#[cfg(windows)]
pub(crate) fn attach(
    image: crate::host::windows::ModuleImage,
    controls: crate::config::Controls,
) -> anyhow::Result<Box<dyn crate::game::GameAdapter>> {
    native::configure_controls(controls.clone());
    native::install(image)?;
    Ok(Box::new(Adapter { controls }))
}

#[cfg(windows)]
struct Adapter {
    controls: crate::config::Controls,
}

#[cfg(windows)]
impl crate::game::GameAdapter for Adapter {
    fn cancel_selection(&self) {
        let mut m = native::MAILBOX.lock().unwrap();
        m.command = None;
        m.ack = None;
    }
    fn set_menu_open(&self, open: bool) {
        native::MENU_OPEN.store(open, std::sync::atomic::Ordering::Release);
    }
    fn configure_controls(&self, controls: &crate::config::Controls) {
        native::configure_controls(controls.clone());
    }
    fn rules(&self) -> &'static dyn crate::game::GameRules {
        &super::RULES
    }
    fn startup_messages(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "input",
                format!(
                    "Opposite Start menu: enabled={} double_tap_ms={}; single-player SP song select only, Start is read-only; panel captures active-side navigation",
                    self.controls.skip_enabled, self.controls.double_tap_ms
                ),
            ),
            (
                "game",
                "Native hooks installed for the verified IIDX 33 build; waiting for song select."
                    .into(),
            ),
            (
                "input",
                format!("Input poll chain: {}", native::input_chain()),
            ),
        ]
    }
    fn catalog(
        &self,
        override_path: &std::path::Path,
    ) -> anyhow::Result<Option<Vec<crate::game::Song>>> {
        if !override_path.as_os_str().is_empty() {
            return catalog::parse_database(&std::fs::read(override_path)?).map(Some);
        }
        native::LIVE_DATABASE
            .get()
            .map(|bytes| catalog::parse_database(bytes))
            .transpose()
    }
    fn take_search_index(&self) -> Option<Vec<(u32, String)>> {
        native::SEARCH_INDEX.lock().unwrap().take()
    }
    fn poll(&self) -> crate::game::GameUpdate {
        use crate::{game::GameUpdate, host::spice};
        let mut m = native::MAILBOX.lock().unwrap();
        let toggle_menu = m
            .menu_event
            .take()
            .is_some_and(|e| m.menu_side == Some(e.side) && m.snapshot.epoch == e.epoch);
        GameUpdate {
            player_card: native::player_card(),
            snapshot: m.snapshot,
            now_playing: m.now_playing.clone(),
            plays: m.plays,
            selection_result: m.ack.take(),
            skip: None,
            toggle_menu,
            navigation: m.navigation.drain(..).collect(),
            input_status: format!(
                "active_side={:?} sdk_status={} (-1=unavailable, -2=not_sampled, 0=ok)",
                m.menu_side.map(|s| s + 1),
                spice::status()
            ),
        }
    }
    fn submit(&self, selection: crate::game::Selection) {
        let mut m = native::MAILBOX.lock().unwrap();
        if !matches!(selection.mode, super::SP | super::DP)
            || selection
                .chart
                .is_some_and(|c| c.mode != selection.mode || super::difficulty(c).is_none())
        {
            m.ack = Some(crate::game::SelectionResult {
                token: selection.token,
                result: Some(Err("谱面不属于当前游戏适配器".into())),
            });
        } else {
            m.command = Some(selection);
        }
    }
    fn set_skip_target(&self, _token: Option<u64>) {}
    fn diagnostics(&self) -> String {
        native::diagnostics()
    }
    fn disabled(&self) -> bool {
        native::DISABLED.load(std::sync::atomic::Ordering::Acquire)
    }
    fn stop(&self) {
        native::DISABLED.store(true, std::sync::atomic::Ordering::Release);
    }
}
