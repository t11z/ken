//! Named Pipe IPC client running in the tray app.
//!
//! Per ADR-0010, the tray app is always the initiator: it connects to
//! the service's named pipe, sends one request, receives one response,
//! and disconnects. The client is synchronous (blocking) because the
//! tray app runs pipe operations on a background thread.

#![cfg(all(windows, feature = "tray-app"))]

use ken_protocol::ids::CommandId;

use crate::ipc::{AgentStatus, IpcRequest, IpcResponse, PendingConsentInfo};

/// Typed client for the Named Pipe IPC channel.
pub struct IpcClient {
    pipe: windows::Win32::Foundation::HANDLE,
}

impl IpcClient {
    /// Connect to the service's named pipe for the current session.
    ///
    /// The session ID is determined via `ProcessIdToSessionId` on the
    /// current process. The pipe name is `\\.\pipe\ken-agent-<session_id>`.
    ///
    /// # Errors
    ///
    /// - "service is not running" if the pipe does not exist.
    /// - "wrong user or ACL mismatch" if access is denied.
    pub fn connect() -> Result<Self, anyhow::Error> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_EXISTING,
        };
        use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        use windows::Win32::System::Threading::GetCurrentProcessId;

        // Determine our session ID
        let mut session_id: u32 = 0;
        unsafe {
            let pid = GetCurrentProcessId();
            ProcessIdToSessionId(pid, &mut session_id)
                .map_err(|e| anyhow::anyhow!("ProcessIdToSessionId failed: {e}"))?;
        }

        let name = super::server::pipe_name(session_id);
        let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        use windows::Win32::System::Pipes::WaitNamedPipeW;

        let handle = unsafe {
            CreateFileW(
                PCWSTR(name_wide.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        };

        match handle {
            Ok(h) if h != INVALID_HANDLE_VALUE => Ok(Self { pipe: h }),
            Ok(_) => Err(anyhow::anyhow!("service is not running")),
            Err(e) => {
                let code = e.code().0 as u32;
                // ERROR_FILE_NOT_FOUND = 2, ERROR_ACCESS_DENIED = 5, ERROR_PIPE_BUSY = 231
                if code == 2 {
                    Err(anyhow::anyhow!("service is not running"))
                } else if code == 5 {
                    Err(anyhow::anyhow!("wrong user or ACL mismatch"))
                } else if code == 231 {
                    // ERROR_PIPE_BUSY: server is handling another client.
                    // Wait up to 500 ms then retry once (issue #75).
                    let _ = unsafe { WaitNamedPipeW(PCWSTR(name_wide.as_ptr()), 500) };
                    let retry = unsafe {
                        CreateFileW(
                            PCWSTR(name_wide.as_ptr()),
                            (GENERIC_READ | GENERIC_WRITE).0,
                            FILE_SHARE_NONE,
                            None,
                            OPEN_EXISTING,
                            FILE_ATTRIBUTE_NORMAL,
                            None,
                        )
                    };
                    match retry {
                        Ok(h) if h != INVALID_HANDLE_VALUE => Ok(Self { pipe: h }),
                        Ok(_) => Err(anyhow::anyhow!("service is not running")),
                        Err(e2) => {
                            let code2 = e2.code().0 as u32;
                            if code2 == 2 {
                                Err(anyhow::anyhow!("service is not running"))
                            } else if code2 == 5 {
                                Err(anyhow::anyhow!("wrong user or ACL mismatch"))
                            } else {
                                Err(anyhow::anyhow!(
                                    "failed to connect to pipe after retry: {e2}"
                                ))
                            }
                        }
                    }
                } else {
                    Err(anyhow::anyhow!("failed to connect to pipe: {e}"))
                }
            }
        }
    }

    /// Send a request and receive a response.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failure or JSON parse failure.
    pub fn send(&mut self, request: &IpcRequest) -> Result<IpcResponse, anyhow::Error> {
        write_message(self.pipe, request)?;
        read_message(self.pipe)
    }

    /// Query the service's current status.
    pub fn get_status(&mut self) -> Result<AgentStatus, anyhow::Error> {
        match self.send(&IpcRequest::GetStatus)? {
            IpcResponse::Status(status) => Ok(status),
            IpcResponse::Error(e) => Err(anyhow::anyhow!("IPC error: {e}")),
            other => Err(anyhow::anyhow!("unexpected response: {other:?}")),
        }
    }

    /// Check if there is a pending consent request.
    ///
    /// Returns `Some(info)` if a consent request is waiting, `None` otherwise.
    pub fn get_pending_consent(&mut self) -> Result<Option<PendingConsentInfo>, anyhow::Error> {
        match self.send(&IpcRequest::GetPendingConsent)? {
            IpcResponse::ConsentPending {
                command_id,
                session_description,
                admin_name,
            } => Ok(Some(PendingConsentInfo {
                command_id,
                session_description,
                admin_name,
            })),
            IpcResponse::NoPendingConsent => Ok(None),
            IpcResponse::Error(e) => Err(anyhow::anyhow!("IPC error: {e}")),
            other => Err(anyhow::anyhow!("unexpected response: {other:?}")),
        }
    }

    /// Report the user's consent decision to the service.
    pub fn submit_consent_response(
        &mut self,
        command_id: &CommandId,
        granted: bool,
    ) -> Result<(), anyhow::Error> {
        match self.send(&IpcRequest::SubmitConsentResponse {
            command_id: *command_id,
            granted,
        })? {
            IpcResponse::ConsentResponseAcknowledged => Ok(()),
            IpcResponse::Error(e) => Err(anyhow::anyhow!("IPC error: {e}")),
            other => Err(anyhow::anyhow!("unexpected response: {other:?}")),
        }
    }

    /// Activate the local kill switch via the service.
    pub fn activate_kill_switch(&mut self) -> Result<(), anyhow::Error> {
        match self.send(&IpcRequest::ActivateKillSwitch)? {
            IpcResponse::KillSwitchActivated => Ok(()),
            IpcResponse::Error(e) => Err(anyhow::anyhow!("IPC error: {e}")),
            other => Err(anyhow::anyhow!("unexpected response: {other:?}")),
        }
    }

    /// Get the tail of the audit log from the service.
    pub fn get_audit_log_tail(&mut self, lines: u32) -> Result<Vec<String>, anyhow::Error> {
        match self.send(&IpcRequest::GetAuditLogTail { lines })? {
            IpcResponse::AuditLogTail(entries) => Ok(entries),
            IpcResponse::Error(e) => Err(anyhow::anyhow!("IPC error: {e}")),
            other => Err(anyhow::anyhow!("unexpected response: {other:?}")),
        }
    }
}

impl Drop for IpcClient {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.pipe);
        }
    }
}

// --------------- wire format helpers ---------------

/// Write a length-prefixed JSON message to a pipe handle.
fn write_message(
    pipe: windows::Win32::Foundation::HANDLE,
    request: &IpcRequest,
) -> Result<(), anyhow::Error> {
    use windows::Win32::Storage::FileSystem::WriteFile;

    let body = serde_json::to_vec(request)?;
    let len = (body.len() as u32).to_le_bytes();

    let mut written: u32 = 0;
    unsafe {
        WriteFile(pipe, Some(&len), Some(&mut written), None)
            .map_err(|e| anyhow::anyhow!("failed to write length prefix: {e}"))?;
        WriteFile(pipe, Some(&body), Some(&mut written), None)
            .map_err(|e| anyhow::anyhow!("failed to write message body: {e}"))?;
    }

    Ok(())
}

/// Read a length-prefixed JSON message from a pipe handle.
fn read_message(pipe: windows::Win32::Foundation::HANDLE) -> Result<IpcResponse, anyhow::Error> {
    use windows::Win32::Storage::FileSystem::ReadFile;

    let mut len_buf = [0u8; 4];
    let mut bytes_read: u32 = 0;
    unsafe {
        ReadFile(pipe, Some(&mut len_buf), Some(&mut bytes_read), None)
            .map_err(|e| anyhow::anyhow!("failed to read length prefix: {e}"))?;
    }
    if bytes_read != 4 {
        return Err(anyhow::anyhow!(
            "incomplete length prefix: {bytes_read} bytes"
        ));
    }

    let msg_len = u32::from_le_bytes(len_buf) as usize;
    if msg_len > 65536 {
        return Err(anyhow::anyhow!("message too large: {msg_len} bytes"));
    }

    let mut body = vec![0u8; msg_len];
    let mut total_read = 0usize;
    while total_read < msg_len {
        let mut chunk_read: u32 = 0;
        unsafe {
            ReadFile(
                pipe,
                Some(&mut body[total_read..]),
                Some(&mut chunk_read),
                None,
            )
            .map_err(|e| anyhow::anyhow!("failed to read message body: {e}"))?;
        }
        total_read += chunk_read as usize;
    }

    let response: IpcResponse = serde_json::from_slice(&body)?;
    Ok(response)
}
