//! Writes a canvas to disk on a background thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use scene::SceneFile;
use scene::file::NapkinView;

use crate::storage;

/// A canvas to serialize with a view and write to `path`.
pub struct SaveJob {
    pub path: PathBuf,
    pub file: Arc<SceneFile>,
    pub view: NapkinView,
}

pub type SaveResult = Result<SystemTime, String>;

/// Serializes with the view and writes atomically.
pub fn save(job: &SaveJob) -> SaveResult {
    let text = job.file.to_json_string_with_view(Some(job.view));
    storage::write_atomic(&job.path, &text)
        .map_err(|error| format!("{}: {error}", job.path.display()))
}

/// Runs [`save`] jobs one at a time on a background thread, in submission order.
pub struct SaveWorker {
    jobs: mpsc::Sender<SaveJob>,
    results: mpsc::Receiver<SaveResult>,
    thread: thread::JoinHandle<()>,
}

impl SaveWorker {
    /// `wake` runs on the worker thread after every job (the app requests a repaint).
    pub fn spawn(wake: impl Fn() + Send + 'static) -> SaveWorker {
        let (job_tx, job_rx) = mpsc::channel::<SaveJob>();
        let (result_tx, result_rx) = mpsc::channel::<SaveResult>();
        let thread = thread::spawn(move || {
            for job in job_rx {
                let result = save(&job);
                let _ = result_tx.send(result);
                wake();
            }
        });
        SaveWorker {
            jobs: job_tx,
            results: result_rx,
            thread,
        }
    }

    pub fn submit(&self, job: SaveJob) {
        // The worker thread only stops once `jobs` is dropped in `shutdown`, so the
        // receiving end always outlives this sender.
        let _ = self.jobs.send(job);
    }

    pub fn try_result(&self) -> Option<SaveResult> {
        self.results.try_recv().ok()
    }

    /// Finishes queued jobs, joins the thread and returns results not yet taken.
    pub fn shutdown(self) -> Vec<SaveResult> {
        drop(self.jobs);
        let _ = self.thread.join();
        self.results.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn saves_in_the_background_and_reports_back() {
        let dir = std::env::temp_dir().join(format!("napkin-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (woken, wake) = mpsc::channel();
        let worker = SaveWorker::spawn(move || {
            let _ = woken.send(());
        });
        let view = NapkinView {
            scroll_x: 3.0,
            scroll_y: 4.0,
            zoom: 2.0,
        };
        let path = dir.join("w.excalidraw");
        worker.submit(SaveJob {
            path: path.clone(),
            file: Arc::new(SceneFile::new()),
            view,
        });
        wake.recv_timeout(Duration::from_secs(10))
            .expect("the worker woke the UI");
        let mtime = worker.try_result().expect("a result").expect("saved");
        let text = std::fs::read_to_string(&path).expect("file written");
        assert_eq!(
            SceneFile::from_json_str(&text)
                .expect("valid")
                .napkin_view(),
            Some(view)
        );
        assert_eq!(storage::modified(&path).expect("stat"), Some(mtime));

        // The path is a non-empty directory, so the final rename fails.
        worker.submit(SaveJob {
            path: dir.clone(),
            file: Arc::new(SceneFile::new()),
            view,
        });
        let results = worker.shutdown();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err(), "{results:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
