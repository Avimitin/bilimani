//! Read-only opposite-Start input. No input overrides or card data are used.
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub token: u64,
    pub epoch: u64,
    /// Active player, 0 = P1, 1 = P2.
    pub side: u8,
}

pub fn single_side(sp: bool, joined: [bool; 2]) -> Option<u8> {
    match (sp, joined) {
        (true, [true, false]) => Some(0),
        (true, [false, true]) => Some(1),
        _ => None,
    }
}

#[derive(Default)]
pub struct DoubleTap {
    target: Option<Target>,
    pressed: bool,
    first: Option<Duration>,
    fired: bool,
}
impl DoubleTap {
    pub const fn new() -> Self {
        Self {
            target: None,
            pressed: false,
            first: None,
            fired: false,
        }
    }
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Sample every selection frame. A changed target requires a fresh release;
    /// holding Start while entering the screen or switching songs cannot count.
    pub fn sample(
        &mut self,
        target: Option<Target>,
        pressed: Option<bool>,
        now: Duration,
        window: Duration,
    ) -> Option<Target> {
        let (Some(target), Some(pressed)) = (target, pressed) else {
            self.reset();
            return None;
        };
        if self.target != Some(target) {
            self.target = Some(target);
            self.pressed = pressed;
            self.first = None;
            self.fired = false;
            return None;
        }
        let down = pressed && !self.pressed;
        self.pressed = pressed;
        if self.fired || !down {
            return None;
        }
        if self
            .first
            .is_some_and(|at| now.saturating_sub(at) <= window)
        {
            self.first = None;
            self.fired = true;
            Some(target)
        } else {
            self.first = Some(now);
            None
        }
    }
}

#[cfg(windows)]
pub(crate) mod sdk {
    use std::sync::{
        RwLock,
        atomic::{AtomicI32, Ordering},
    };
    pub type GetButton = unsafe extern "C" fn(u32, *mut bool, *mut f32) -> i32;
    static GET_BUTTON: RwLock<Option<GetButton>> = RwLock::new(None);
    static STATUS: AtomicI32 = AtomicI32::new(-1);
    // SPICE_SDK_IIDX_BUTTONS, SDK v0.1 spicesdk_io.h.
    const START: [u32; 2] = [14, 26];

    pub fn set(get: Option<GetButton>) {
        *GET_BUTTON.write().unwrap() = get;
        STATUS.store(if get.is_some() { -2 } else { -1 }, Ordering::Relaxed);
    }
    pub fn status() -> i32 {
        STATUS.load(Ordering::Relaxed)
    }
    pub fn opposite_start(side: u8) -> Option<bool> {
        let button = *START.get(usize::from(side ^ 1))?;
        // Keep the read guard through the call; shutdown clears it before returning.
        let api = GET_BUTTON.read().unwrap();
        let get = api.as_ref()?;
        let mut pressed = false;
        let result = unsafe { get(button, &mut pressed, std::ptr::null_mut()) };
        STATUS.store(result, Ordering::Relaxed);
        (result == 0).then_some(pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            token: 5,
            epoch: 2,
            side: 0,
        }
    }
    fn sample(d: &mut DoubleTap, t: Target, pressed: bool, ms: u64) -> Option<Target> {
        d.sample(
            Some(t),
            Some(pressed),
            Duration::from_millis(ms),
            Duration::from_millis(400),
        )
    }
    #[test]
    fn double_tap_requires_release_and_fires_once_per_request() {
        let mut d = DoubleTap::new();
        let t = target();
        assert_eq!(sample(&mut d, t, true, 0), None); // Held on entry.
        assert_eq!(sample(&mut d, t, true, 20), None);
        sample(&mut d, t, false, 30);
        assert_eq!(sample(&mut d, t, true, 40), None);
        assert_eq!(sample(&mut d, t, true, 100), None);
        sample(&mut d, t, false, 120);
        assert_eq!(sample(&mut d, t, true, 440), Some(t));
        sample(&mut d, t, false, 460);
        assert_eq!(sample(&mut d, t, true, 480), None);
    }
    #[test]
    fn timeout_and_context_changes_do_not_complete_old_tap() {
        let mut d = DoubleTap::new();
        let t = target();
        sample(&mut d, t, false, 0);
        sample(&mut d, t, true, 10);
        sample(&mut d, t, false, 20);
        assert_eq!(sample(&mut d, t, true, 411), None);
        sample(&mut d, t, false, 420);
        assert_eq!(sample(&mut d, t, true, 430), Some(t));
        for next in [
            Target { token: 6, ..t },
            Target { epoch: 3, ..t },
            Target { side: 1, ..t },
        ] {
            d.reset();
            sample(&mut d, t, false, 0);
            sample(&mut d, t, true, 10);
            sample(&mut d, t, false, 20);
            assert_eq!(sample(&mut d, next, true, 30), None);
            sample(&mut d, next, false, 40);
            assert_eq!(sample(&mut d, next, true, 50), None);
        }
        for (context, state) in [(None, Some(false)), (Some(t), None)] {
            d.reset();
            sample(&mut d, t, false, 0);
            sample(&mut d, t, true, 10);
            d.sample(
                context,
                state,
                Duration::from_millis(20),
                Duration::from_millis(400),
            );
            sample(&mut d, t, false, 30);
            assert_eq!(sample(&mut d, t, true, 40), None);
        }
    }
    #[test]
    fn only_single_player_sp_has_an_opposite_side() {
        assert_eq!(single_side(true, [true, false]), Some(0));
        assert_eq!(single_side(true, [false, true]), Some(1));
        for flags in [[false, false], [true, true]] {
            assert_eq!(single_side(true, flags), None);
        }
        for flags in [[true, false], [false, true], [true, true]] {
            assert_eq!(single_side(false, flags), None);
        }
    }
    #[cfg(windows)]
    #[test]
    fn sdk_reads_opposite_start_and_handles_missing_or_failed_getter() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static BUTTON: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn get(id: u32, pressed: *mut bool, velocity: *mut f32) -> i32 {
            assert!(velocity.is_null());
            BUTTON.store(id, Ordering::Relaxed);
            unsafe {
                *pressed = true;
            }
            0
        }
        unsafe extern "C" fn fail(_: u32, _: *mut bool, _: *mut f32) -> i32 {
            7
        }
        sdk::set(Some(get));
        assert_eq!(sdk::opposite_start(0), Some(true));
        assert_eq!(BUTTON.load(Ordering::Relaxed), 26);
        assert_eq!(sdk::opposite_start(1), Some(true));
        assert_eq!(BUTTON.load(Ordering::Relaxed), 14);
        sdk::set(Some(fail));
        assert_eq!(sdk::opposite_start(0), None);
        assert_eq!(sdk::status(), 7);
        sdk::set(None);
        assert_eq!(sdk::opposite_start(0), None);
    }
}
