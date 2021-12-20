// Copyright 2021 TiKV Project Authors. Licensed under Apache-2.0.

//! this mod could help you to upload profiler data to the pyroscope
//!
//! To enable this mod, you need to enable the features: "pyroscope" and
//! "default-tls" (or "rustls-tls"). To start profiling, you can create a
//! `PyroscopeAgent`:
//!
//! ```ignore
//! let guard =  
//!   PyroscopeAgentBuilder::new("http://localhost:4040".to_owned(), "fibonacci".to_owned())
//!     .frequency(99)
//!     .tags([
//!         ("TagA".to_owned(), "ValueA".to_owned()),
//!         ("TagB".to_owned(), "ValueB".to_owned()),
//!     ]
//!     .iter()
//!     .cloned()
//!     .collect())
//!     .build().unwrap();
//! ```
//!
//! This guard will collect profiling data and send profiling data to the
//! pyroscope server every 10 seconds. This interval is not configurable now
//! (both server side and client side).
//!
//! If you need to stop the profiling, you can call `stop()` on the guard:
//!
//! ```ignore
//! guard.stop().await
//! ```
//!
//! It will return the error if error occurs while profiling.

use std::collections::HashMap;

use pprof::ProfilerGuardBuilder;
use pprof::Result;
use pprof::report::Report;

use tokio::sync::mpsc;

use libc::c_int;

pub struct PyroscopeAgentBuilder {
    inner_builder: ProfilerGuardBuilder,

    url: String,
    application_name: String,
    tags: HashMap<String, String>,
}

impl PyroscopeAgentBuilder {
    pub fn new<S: AsRef<str>>(url: S, application_name: S) -> Self {
        Self {
            inner_builder: ProfilerGuardBuilder::default(),
            url: url.as_ref().to_owned(),
            application_name: application_name.as_ref().to_owned(),
            tags: HashMap::new(),
        }
    }

    pub fn frequency(self, frequency: c_int) -> Self {
        Self {
            inner_builder: self.inner_builder.frequency(frequency),
            ..self
        }
    }

    pub fn blocklist<T: AsRef<str>>(self, blocklist: &[T]) -> Self {
        Self {
            inner_builder: self.inner_builder.blocklist(blocklist),
            ..self
        }
    }

    pub fn tags(self, tags: HashMap<String, String>) -> Self {
        Self { tags, ..self }
    }

    pub fn build(self) -> Result<PyroscopeAgent> {
        let application_name = merge_tags_with_app_name(self.application_name, self.tags);
        let (stopper, mut stop_signal) = mpsc::channel::<()>(1);

        // Since Pyroscope only allow 10s intervals, it might not be necessary
        // to make this customizable at this point
        let upload_interval = std::time::Duration::from_secs(10);
        let mut interval = tokio::time::interval(upload_interval);

        let handler = tokio::spawn(async move {
            loop {
                match self.inner_builder.clone().build() {
                    Ok(guard) => {
                        tokio::select! {
                            _ = interval.tick() => {
                                pyroscope_ingest(guard.report().build()?, &self.url, &application_name).await?;
                            }
                            _ = stop_signal.recv() => {
                                pyroscope_ingest(guard.report().build()?, &self.url, &application_name).await?;

                                break Ok(())
                            }
                        }
                    }
                    Err(err) => {
                        // TODO: this error will only be caught when this
                        // handler is joined. Find way to report error earlier
                        break Err(err);
                    }
                }
            }
        });

        Ok(PyroscopeAgent { stopper, handler })
    }
}

pub struct PyroscopeAgent {
    stopper: mpsc::Sender<()>,

    handler: tokio::task::JoinHandle<Result<()>>,
}

impl PyroscopeAgent {
    pub async fn stop(self) -> Result<()> {
        self.stopper.send(()).await.unwrap();

        self.handler.await.unwrap()?;

        Ok(())
    }
}

async fn pyroscope_ingest<S: AsRef<str>, N: AsRef<str>>(
            report: Report,
            url: S,
            application_name: N,
        ) -> Result<()> {
            let mut buffer = Vec::new();

            report.fold(true, &mut buffer)?;

            if buffer.is_empty() {
                return Ok(());
            }

            let client = reqwest::Client::new();
            // TODO: handle the error of this request

            let start: u64 = report 
                .timing
                .start_time
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let s_start = start - start.checked_rem(10).unwrap();
            // This assumes that the interval between start and until doesn't
            // exceed 10s
            let s_until = s_start + 10;

            client
                .post(format!("{}/ingest", url.as_ref()))
                .header("Content-Type", "binary/octet-stream")
                .query(&[
                    ("name", application_name.as_ref()),
                    ("from", &format!("{}", s_start)),
                    ("until", &format!("{}", s_until)),
                    ("format", "folded"),
                    ("sampleRate", &format!("{}", report.sample_rate)),
                    ("spyName", "pprof-rs"),
                ])
                .body(buffer)
                .send()
                .await?;

            Ok(())
        }

fn merge_tags_with_app_name(application_name: String, tags: HashMap<String, String>) -> String {
    let mut tags_vec = tags
        .into_iter()
        .filter(|(k, _)| k != "__name__")
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<String>>();
    tags_vec.sort();
    let tags_str = tags_vec.join(",");

    if !tags_str.is_empty() {
        format!("{}{{{}}}", application_name, tags_str,)
    } else {
        application_name
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::pyroscope::merge_tags_with_app_name;

    #[test]
    fn merge_tags_with_app_name_with_tags() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "staging".to_string());
        tags.insert("region".to_string(), "us-west-1".to_string());
        tags.insert("__name__".to_string(), "reserved".to_string());
        assert_eq!(
            merge_tags_with_app_name("my.awesome.app.cpu".to_string(), tags),
            "my.awesome.app.cpu{env=staging,region=us-west-1}".to_string()
        )
    }

    #[test]
    fn merge_tags_with_app_name_without_tags() {
        assert_eq!(
            merge_tags_with_app_name("my.awesome.app.cpu".to_string(), HashMap::default()),
            "my.awesome.app.cpu".to_string()
        )
    }
}
