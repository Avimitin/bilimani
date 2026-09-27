//! Reconfigure transport without blocking the queue worker or overlapping sessions.
use super::{Event, SourceConfig, create};
use std::time::{Duration, Instant};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
pub struct Session {
    task: JoinHandle<()>,
    stop: watch::Sender<bool>,
    events: mpsc::Receiver<Event>,
    pending: Option<(SourceConfig, Instant)>,
}
impl Session {
    pub fn new(config: SourceConfig) -> Self {
        Self::with_source(create(config))
    }
    fn with_source(source: Box<dyn super::ChatSource>) -> Self {
        let (tx, events) = mpsc::channel(512);
        let (stop, rx) = watch::channel(false);
        let task = tokio::spawn(source.run(tx, rx));
        Self {
            task,
            stop,
            events,
            pending: None,
        }
    }
    pub fn replace(&mut self, config: SourceConfig) {
        let _ = self.stop.send(true);
        // New settings replace the pending destination, without extending teardown.
        let began = self.pending.as_ref().map_or_else(Instant::now, |p| p.1);
        self.pending = Some((config, began));
    }
    pub fn poll(&mut self) {
        if let Some((_, began)) = &self.pending {
            while self.events.try_recv().is_ok() {} // Never process messages from the old room.
            if began.elapsed() >= Duration::from_secs(18) {
                self.task.abort();
            }
            if self.task.is_finished() {
                let (config, _) = self.pending.take().unwrap();
                *self = Self::new(config);
            }
        }
    }
    pub fn try_recv(&mut self) -> Option<Event> {
        if self.pending.is_some() {
            None
        } else {
            self.events.try_recv().ok()
        }
    }
    pub async fn stop(&mut self) {
        let _ = self.stop.send(true);
        if tokio::time::timeout(Duration::from_secs(18), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Bilibili,
        platforms::{Chat, ChatSource},
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    struct SlowSource(Arc<AtomicBool>);
    impl ChatSource for SlowSource {
        fn run(
            self: Box<Self>,
            tx: mpsc::Sender<Event>,
            mut stop: watch::Receiver<bool>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            Box::pin(async move {
                let _ = stop.changed().await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                let _ = tx
                    .send(Event::RoomInfo(super::super::RoomInfo {
                        room_id: 123,
                        name: "old anchor".into(),
                        title: "must not appear in new profile".into(),
                    }))
                    .await;
                let _ = tx
                    .send(Event::Chat(Chat {
                        user: "old room".into(),
                        name: String::new(),
                        text: "must not reach engine".into(),
                    }))
                    .await;
                self.0.store(true, Ordering::Release);
            })
        }
    }
    #[tokio::test]
    async fn reconnection_waits_for_old_session_and_discards_its_remaining_messages() {
        let ended = Arc::new(AtomicBool::new(false));
        let mut session = Session::with_source(Box::new(SlowSource(ended.clone())));
        session.replace(SourceConfig::Bilibili(Bilibili {
            enabled: false,
            ..Default::default()
        }));
        session.poll();
        assert!(session.pending.is_some());
        assert!(session.try_recv().is_none());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ended.load(Ordering::Acquire));
        session.poll();
        assert!(session.pending.is_none());
        tokio::task::yield_now().await;
        assert!(matches!(session.try_recv(), Some(Event::Status(_))));
        assert!(session.try_recv().is_none());
        session.stop().await;
    }
}
