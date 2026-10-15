//! Bounded read-only path observation. No filesystem call runs on the executor.
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) const MAX_OBSERVERS: usize = 16;
pub(crate) struct Observation {
    pub id: Uuid,
    pub path: PathBuf,
    pub deadline: Instant,
}
pub(crate) struct Completion {
    pub id: Uuid,
    pub result: Result<(), &'static str>,
}
pub(crate) struct PathObservers {
    sender: SyncSender<Observation>,
    receiver: Receiver<Completion>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl PathObservers {
    pub fn start() -> Result<Self, String> {
        let (sender, requests) = mpsc::sync_channel(MAX_OBSERVERS);
        let (completed, receiver) = mpsc::sync_channel(MAX_OBSERVERS);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new()
            .name("zmux-download-paths".into())
            .spawn(move || {
                let mut active = VecDeque::new();
                while !stopping.load(Ordering::Acquire) {
                    while let Ok(observation) = requests.try_recv() {
                        active.push_back(observation);
                    }
                    let count = active.len();
                    for _ in 0..count {
                        let Some(observation): Option<Observation> = active.pop_front() else {
                            break;
                        };
                        // Queued observations never begin IO after their deadline.
                        let outcome = if Instant::now() >= observation.deadline {
                            Some(Err("timeout"))
                        } else if !observation.path.parent().is_some_and(|p| p.is_dir()) {
                            Some(Err("download_path_watch_failed"))
                        } else if Instant::now() >= observation.deadline {
                            Some(Err("timeout"))
                        } else if std::fs::metadata(&observation.path).is_ok_and(|m| m.len() > 0) {
                            Some(Ok(()))
                        } else {
                            None
                        };
                        if let Some(result) = outcome {
                            if completed
                                .send(Completion {
                                    id: observation.id,
                                    result,
                                })
                                .is_err()
                            {
                                return;
                            }
                        } else {
                            active.push_back(observation);
                        }
                    }
                    // One physical worker and at most sixteen charged observations,
                    // including queued/completed work. Blocking storage cannot spawn
                    // replacement threads or release the caller's capacity credit.
                    thread::sleep(Duration::from_millis(50));
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sender,
            receiver,
            stop,
            worker: Some(worker),
        })
    }
    pub fn submit(&self, observation: Observation) -> Result<(), String> {
        self.sender
            .try_send(observation)
            .map_err(|error| error.to_string())
    }
    pub fn completion(&self) -> Result<Option<Completion>, String> {
        match self.receiver.try_recv() {
            Ok(completion) => Ok(Some(completion)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err("download path observer stopped".into()),
        }
    }
    pub fn shutdown(&mut self, deadline: Instant) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        while self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            if Instant::now() >= deadline {
                return Err(
                    "download path observer shutdown timed out; IO may still be running".into(),
                );
            }
            thread::sleep(Duration::from_millis(5));
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "download path observer panicked".to_string())?;
        }
        Ok(())
    }
}
impl Drop for PathObservers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Never synchronously join a possibly blocked storage call in Drop.
    }
}
