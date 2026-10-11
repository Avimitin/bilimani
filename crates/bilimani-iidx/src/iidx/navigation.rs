//! Controller edge/repeat policy, independent of native memory and egui.
use crate::game::Navigation;
#[derive(Default)]
pub struct Navigator {
    context: Option<(u64, u8)>,
    held: u32,
    next_repeat: [u64; 2],
    scratch_at: u64,
    suppressed: u32,
}
impl Navigator {
    pub fn sample(
        &mut self,
        context: Option<(u64, u8)>,
        buttons: u32,
        scratch: i32,
        now_ms: u64,
    ) -> (Vec<Navigation>, u32) {
        let mut events = Vec::new();
        let release_mask = self.suppressed;
        self.suppressed &= buttons;
        let Some((_, side)) = context else {
            self.context = None;
            self.held = buttons;
            return (events, release_mask);
        };
        let mask = 0x7f << (side * 7);
        self.suppressed |= buttons & mask;
        if self.context != context {
            self.context = context;
            self.held = buttons;
            self.next_repeat = [now_ms + 400; 2];
            self.scratch_at = now_ms;
            return (events, mask | release_mask);
        }
        for (i, (bit, event)) in [
            (0, Navigation::Down),
            (1, Navigation::Up),
            (5, Navigation::Confirm),
            (6, Navigation::Back),
        ]
        .into_iter()
        .enumerate()
        {
            let bit = 1 << (bit + side * 7);
            if buttons & bit == 0 {
                continue;
            }
            if self.held & bit == 0 {
                events.push(event);
                if i < 2 {
                    self.next_repeat[i] = now_ms + 400;
                }
            } else if i < 2 && now_ms >= self.next_repeat[i] {
                events.push(event);
                self.next_repeat[i] = now_ms + 100;
            }
        }
        if scratch != 0 && now_ms.saturating_sub(self.scratch_at) >= 80 {
            events.push(if scratch < 0 {
                Navigation::Left
            } else {
                Navigation::Right
            });
            self.scratch_at = now_ms;
        }
        self.held = buttons;
        (events, mask | release_mask)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_side_edges_repeat_and_close_release_guard() {
        let mut n = Navigator::default();
        assert!(n.sample(Some((1, 1)), 0, 0, 0).0.is_empty());
        assert!(n.sample(Some((1, 1)), 1, 0, 1).0.is_empty()); // P1 ignored
        assert_eq!(n.sample(Some((1, 1)), 1 << 7, 0, 10).0, [Navigation::Down]);
        assert!(n.sample(Some((1, 1)), 1 << 7, 0, 409).0.is_empty());
        assert_eq!(n.sample(Some((1, 1)), 1 << 7, 0, 410).0, [Navigation::Down]);
        assert_eq!(
            n.sample(Some((1, 1)), 1 << 12, -1, 500).0,
            [Navigation::Confirm, Navigation::Left]
        );
        assert!(n.sample(Some((1, 1)), 1 << 12, 1, 501).0.is_empty());
        assert_eq!(n.sample(None, 1 << 12, 0, 520).1, 1 << 12);
        assert_eq!(n.sample(None, 0, 0, 540).1, 1 << 12);
        assert_eq!(n.sample(None, 0, 0, 550).1, 0);
        assert!(n.sample(Some((2, 1)), 1 << 12, 0, 560).0.is_empty()); // held on reopen
    }
}
