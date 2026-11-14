//! Bounded directory reads with ownership of every opened remote handle.
use std::sync::Arc;

use super::SftpSession;
use crate::client::{error::Error, fs::ReadDir, rawsession::SftpResult, runtime, RawSftpSession};
use crate::protocol::StatusCode;

/// `complete` is true only after EOF; reaching a budget does not probe for EOF.
pub struct LimitedReadDir {
    pub entries: ReadDir,
    pub complete: bool,
}

struct DirectoryHandle {
    session: Arc<RawSftpSession>,
    handle: Option<String>,
}

impl DirectoryHandle {
    async fn close(mut self) -> SftpResult<()> {
        if let Some(handle) = self.handle.take() {
            self.session.close(handle).await?;
        }
        Ok(())
    }
}

impl Drop for DirectoryHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.session.close_detached(handle);
        }
    }
}

impl SftpSession {
    /// Reads at most `max_entries` wire entries and `max_name_bytes` filename
    /// bytes. One already received NAME packet may contain additional entries;
    /// its size remains bounded by the configured incoming packet limit.
    /// No further READDIR is sent when either budget is reached.
    ///
    /// Dropping this future closes its directory, including an OPENDIR whose
    /// reply arrives after cancellation. Existing unbounded APIs are unchanged.
    pub async fn read_dir_bytes_limited<P: Into<Vec<u8>>>(
        &self,
        path: P,
        max_entries: usize,
        max_name_bytes: usize,
    ) -> SftpResult<LimitedReadDir> {
        let path = path.into();
        let mut result = LimitedReadDir {
            entries: ReadDir {
                parent: Arc::from(path.as_slice()),
                entries: Default::default(),
            },
            complete: false,
        };
        if max_entries == 0 || max_name_bytes == 0 {
            return Ok(result);
        }
        // A dropped OPENDIR request otherwise loses the server's returned
        // handle. The response owner outlives the caller and closes a late reply.
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let session = self.session.clone();
        runtime::spawn(async move {
            let opened = session
                .opendir_bytes(path)
                .await
                .map(|opened| DirectoryHandle {
                    session: session.clone(),
                    handle: Some(opened.handle),
                });
            let _ = sender.send(opened);
        });
        let directory = receiver
            .await
            .map_err(|_| Error::UnexpectedBehavior("directory opener stopped".into()))??;
        let mut bytes = 0usize;
        'listing: loop {
            match directory
                .session
                .readdir(directory.handle.as_deref().expect("open directory"))
                .await
            {
                Ok(name) => {
                    // Even empty/nonconforming NAME replies are bounded by the
                    // caller's operation deadline, not an unbounded allocation.
                    for file in name.files {
                        if file.filename.len() > max_name_bytes - bytes {
                            break 'listing;
                        }
                        bytes += file.filename.len();
                        result
                            .entries
                            .entries
                            .push_back((file.filename, file.attrs));
                        if result.entries.entries.len() == max_entries || bytes == max_name_bytes {
                            break 'listing;
                        }
                    }
                }
                Err(Error::Status(status)) if status.status_code == StatusCode::Eof => {
                    result.complete = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        directory.close().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
