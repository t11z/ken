//! Named Pipe IPC server running in the SYSTEM service.
//!
//! Per ADR-0010, the pipe server accepts connections from the tray app
//! and handles status queries, consent polling, audit log retrieval,
//! and kill-switch activation. The pipe is secured with an explicit
//! ACL restricting access to the interactive user and SYSTEM.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ken_protocol::audit::AuditEventKind;
use ken_protocol::ids::CommandId;
use tokio::sync::oneshot;

use crate::audit::AuditLogger;
use crate::ipc::{AgentStatus, IpcRequest, IpcResponse};

/// A pending consent request waiting for the tray app to respond.
pub struct PendingConsentRequest {
    /// Which command this consent request is for.
    pub command_id: CommandId,
    /// Description of why the session is requested.
    pub session_description: String,
    /// Who is requesting the session.
    pub admin_name: String,
    /// Channel to send the user's decision back to the command processor.
    pub response_tx: oneshot::Sender<bool>,
}

/// Shared state between the pipe server and the command processor.
pub type SharedConsentState = Arc<Mutex<Option<PendingConsentRequest>>>;

/// Create a new shared consent state.
#[must_use]
pub fn new_consent_state() -> SharedConsentState {
    Arc::new(Mutex::new(None))
}

/// Construct the pipe name for the given session ID.
///
/// Per ADR-0010, the pipe name includes the session ID to ensure
/// the tray app connects to the correct service instance.
#[must_use]
pub fn pipe_name(session_id: u32) -> String {
    format!(r"\\.\pipe\ken-agent-{session_id}")
}

/// Run the Named Pipe server loop.
///
/// This function blocks and should be called via
/// `tokio::task::spawn_blocking`. It creates a named pipe, sets
/// up security, and loops accepting connections.
#[allow(clippy::too_many_lines)] // Windows pipe server loop: the connection lifecycle is clearest as a single function
pub fn run(
    consent_state: SharedConsentState,
    shutdown: Arc<AtomicBool>,
    audit: Arc<AuditLogger>,
    paths: Arc<crate::config::DataPaths>,
) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
        PIPE_TYPE_MESSAGE, PIPE_WAIT,
    };
    use windows::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;

    let session_id = unsafe { WTSGetActiveConsoleSessionId() };
    if session_id == 0xFFFF_FFFF {
        tracing::warn!("no active console session, pipe server not starting");
        return;
    }

    let name = pipe_name(session_id);
    tracing::info!(pipe = %name, session_id, "starting IPC pipe server");

    // Build security descriptor with explicit ACL restricting access
    // to the interactive session user and SYSTEM.
    let sd_holder = match build_security_descriptor(session_id) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "failed to build pipe security descriptor");
            return;
        }
    };

    let sa = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>())
            .expect("SECURITY_ATTRIBUTES size fits u32"),
        lpSecurityDescriptor: sd_holder.as_ptr(),
        bInheritHandle: false.into(),
    };

    let pipe_name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    while !shutdown.load(Ordering::SeqCst) {
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_name_wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                1,    // max instances: one connection at a time
                8192, // output buffer size
                8192, // input buffer size
                0,    // default timeout
                Some(&sa),
            )
        };

        if pipe == INVALID_HANDLE_VALUE {
            tracing::error!("failed to create named pipe");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        // Block until a client connects. ERROR_PIPE_CONNECTED (535)
        // means the client connected between Create and Connect — OK.
        if let Err(e) = unsafe { ConnectNamedPipe(pipe, None) } {
            let os_err = std::io::Error::last_os_error();
            if os_err.raw_os_error() != Some(535) {
                tracing::warn!(
                    error = %e,
                    "ConnectNamedPipe failed"
                );
                unsafe {
                    let _ = CloseHandle(pipe);
                }
                continue;
            }
        }

        // One request/response exchange per connection per ADR-0010.
        match read_message(pipe) {
            Ok(request) => {
                let response = dispatch(&request, &consent_state, &audit, &paths);

                if let Err(e) = write_message(pipe, &response) {
                    tracing::warn!(error = %e, "failed to write IPC response");
                }

                // Kill switch: signal shutdown AFTER the response is sent
                // so the tray app sees the success confirmation.
                if matches!(request, IpcRequest::ActivateKillSwitch)
                    && matches!(response, IpcResponse::KillSwitchActivated)
                {
                    tracing::info!("kill switch activated via IPC, signalling shutdown");
                    shutdown.store(true, Ordering::SeqCst);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to read IPC request");
            }
        }

        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
    }

    tracing::info!("pipe server shutting down");
}

// --------------- request dispatch ---------------

/// Route an IPC request to its handler.
fn dispatch(
    request: &IpcRequest,
    consent_state: &SharedConsentState,
    audit: &AuditLogger,
    paths: &crate::config::DataPaths,
) -> IpcResponse {
    match request {
        IpcRequest::GetStatus => handle_get_status(paths),
        IpcRequest::GetPendingConsent => handle_get_pending_consent(consent_state),
        IpcRequest::SubmitConsentResponse {
            command_id,
            granted,
        } => handle_submit_consent(consent_state, command_id, *granted, audit),
        IpcRequest::GetAuditLogTail { lines } => handle_get_audit_tail(audit, *lines),
        IpcRequest::ActivateKillSwitch => handle_kill_switch(paths, audit),
    }
}

fn handle_get_status(paths: &crate::config::DataPaths) -> IpcResponse {
    let endpoint_id = std::fs::read_to_string(&paths.endpoint_id_file)
        .ok()
        .map(|s| s.trim().to_string());
    let enrolled = endpoint_id.is_some();

    IpcResponse::Status(AgentStatus {
        service_running: true,
        enrolled,
        endpoint_id,
        last_heartbeat: None,
        pending_commands: 0,
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn handle_get_pending_consent(consent_state: &SharedConsentState) -> IpcResponse {
    let guard = consent_state.lock().unwrap_or_else(|e| e.into_inner());
    match &*guard {
        Some(pending) => IpcResponse::ConsentPending {
            command_id: pending.command_id,
            session_description: pending.session_description.clone(),
            admin_name: pending.admin_name.clone(),
        },
        None => IpcResponse::NoPendingConsent,
    }
}

fn handle_submit_consent(
    consent_state: &SharedConsentState,
    command_id: &CommandId,
    granted: bool,
    audit: &AuditLogger,
) -> IpcResponse {
    let mut guard = consent_state.lock().unwrap_or_else(|e| e.into_inner());
    match guard.take() {
        Some(pending) if pending.command_id == *command_id => {
            let kind = if granted {
                AuditEventKind::ConsentGranted
            } else {
                AuditEventKind::ConsentDenied
            };
            audit.log(
                kind,
                &format!(
                    "user {} consent for command {}",
                    if granted { "granted" } else { "denied" },
                    command_id
                ),
            );

            // Forward the decision to the command processor.
            // If the receiver was dropped (timeout), send fails
            // silently — that is acceptable.
            let _ = pending.response_tx.send(granted);
            IpcResponse::ConsentResponseAcknowledged
        }
        Some(pending) => {
            // Wrong command_id — put the pending request back.
            *guard = Some(pending);
            IpcResponse::Error("no matching pending consent request".into())
        }
        None => IpcResponse::Error("no matching pending consent request".into()),
    }
}

fn handle_get_audit_tail(audit: &AuditLogger, lines: u32) -> IpcResponse {
    let events = audit.recent(usize::from(lines));
    let strings: Vec<String> = events
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .collect();
    IpcResponse::AuditLogTail(strings)
}

/// Handle kill-switch activation: write state file, disable service,
/// and respond before signalling shutdown.
fn handle_kill_switch(paths: &crate::config::DataPaths, audit: &AuditLogger) -> IpcResponse {
    let user = "tray-app-user";

    if let Err(e) =
        crate::killswitch::activate(&paths.kill_switch_file, "user-triggered via tray app", user)
    {
        return IpcResponse::Error(format!("failed to activate kill switch: {e}"));
    }

    audit.log(
        AuditEventKind::KillSwitchActivated,
        "kill switch activated via IPC tray app request",
    );

    // Disable the service so it does not restart. Best-effort.
    if let Err(e) = disable_service() {
        tracing::warn!(error = %e, "failed to set service to disabled");
    }

    IpcResponse::KillSwitchActivated
}

/// Set the Ken Agent service startup type to Disabled per ADR-0012.
fn disable_service() -> Result<(), anyhow::Error> {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        crate::service::lifecycle::SERVICE_NAME,
        ServiceAccess::CHANGE_CONFIG,
    )?;

    let config = windows_service::service::ServiceInfo {
        name: std::ffi::OsString::from(crate::service::lifecycle::SERVICE_NAME),
        display_name: std::ffi::OsString::from(crate::service::lifecycle::SERVICE_DISPLAY_NAME),
        service_type: windows_service::service::ServiceType::OWN_PROCESS,
        start_type: windows_service::service::ServiceStartType::Disabled,
        error_control: windows_service::service::ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec![std::ffi::OsString::from("run-service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    service.change_config(&config)?;

    tracing::info!("service startup type set to Disabled");
    Ok(())
}

// --------------- security descriptor ---------------

/// Holds a SECURITY_DESCRIPTOR and the ACL buffer it references.
/// The ACL must live as long as the descriptor uses it.
struct SecurityDescriptorHolder {
    _sd_buffer: Vec<u8>,
    _acl_buffer: Vec<u8>,
    _user_sid_buffer: Vec<u8>,
    _system_sid_buffer: Vec<u8>,
    sd_ptr: *mut std::ffi::c_void,
}

// SAFETY: The holder owns all buffers and is used only by the pipe
// server thread. The raw pointers point into owned Vec allocations.
unsafe impl Send for SecurityDescriptorHolder {}
unsafe impl Sync for SecurityDescriptorHolder {}

impl SecurityDescriptorHolder {
    fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.sd_ptr
    }
}

/// Build a `SECURITY_DESCRIPTOR` granting the session user
/// `GENERIC_READ | GENERIC_WRITE` and SYSTEM full control.
#[allow(clippy::too_many_lines)] // Complex Windows security setup; splitting further reduces clarity
fn build_security_descriptor(session_id: u32) -> Result<SecurityDescriptorHolder, anyhow::Error> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS, TRUSTEE_IS_SID,
        TRUSTEE_IS_USER, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        AllocateAndInitializeSid, CopySid, FreeSid, GetLengthSid, GetTokenInformation,
        InitializeSecurityDescriptor, SetSecurityDescriptorDacl, TokenUser, PSECURITY_DESCRIPTOR,
        PSID, SID_IDENTIFIER_AUTHORITY, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_USER,
    };
    use windows::Win32::System::RemoteDesktop::WTSQueryUserToken;
    use windows::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;

    // --- get user SID from session token ---
    let mut user_token = windows::Win32::Foundation::HANDLE::default();
    unsafe {
        WTSQueryUserToken(session_id, &mut user_token)
            .map_err(|e| anyhow::anyhow!("WTSQueryUserToken failed: {e}"))?;
    }

    let mut user_sid_buffer = unsafe {
        let mut needed: u32 = 0;
        let _ = GetTokenInformation(user_token, TokenUser, None, 0, &mut needed);

        let mut buf = vec![0u8; usize::from(needed)];
        GetTokenInformation(
            user_token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .map_err(|e| {
            let _ = CloseHandle(user_token);
            anyhow::anyhow!("GetTokenInformation failed: {e}")
        })?;

        let _ = CloseHandle(user_token);

        let token_user = &*(buf.as_ptr().cast::<TOKEN_USER>());
        let sid = token_user.User.Sid;
        // GetLengthSid returns u32; keep as u32 to pass directly to CopySid
        let sid_len = GetLengthSid(sid);
        let mut sid_copy = vec![0u8; usize::from(sid_len)];
        CopySid(sid_len, PSID(sid_copy.as_mut_ptr().cast()), sid)
            .map_err(|e| anyhow::anyhow!("CopySid failed: {e}"))?;
        sid_copy
    };

    // --- build well-known SYSTEM SID (S-1-5-18) ---
    let mut system_sid_buffer = unsafe {
        let nt_authority = SID_IDENTIFIER_AUTHORITY {
            Value: [0, 0, 0, 0, 0, 5],
        };
        let mut sid = PSID::default();
        AllocateAndInitializeSid(
            &nt_authority,
            1,
            18, // SECURITY_LOCAL_SYSTEM_RID
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut sid,
        )
        .map_err(|e| anyhow::anyhow!("AllocateAndInitializeSid failed: {e}"))?;

        // GetLengthSid returns u32; keep as u32 to pass directly to CopySid
        let sid_len = GetLengthSid(sid);
        let mut sid_copy = vec![0u8; usize::from(sid_len)];
        CopySid(sid_len, PSID(sid_copy.as_mut_ptr().cast()), sid).map_err(|e| {
            FreeSid(sid);
            anyhow::anyhow!("CopySid for SYSTEM SID failed: {e}")
        })?;
        FreeSid(sid);
        sid_copy
    };

    // --- build ACL with two ACEs ---
    let entries = [
        EXPLICIT_ACCESS_W {
            grfAccessPermissions: 0xC000_0000, // GENERIC_READ | GENERIC_WRITE
            grfAccessMode: SET_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                // ptstrName is typed as PWSTR but the trustee API uses it as
                // a tagged pointer for SID data — alignment is intentional.
                ptstrName: windows::core::PWSTR(user_sid_buffer.as_mut_ptr().cast::<u16>()),
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            },
        },
        EXPLICIT_ACCESS_W {
            grfAccessPermissions: 0x001F_01FF, // FILE_ALL_ACCESS
            grfAccessMode: SET_ACCESS,
            grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                // ptstrName is typed as PWSTR but the trustee API uses it as
                // a tagged pointer for SID data — alignment is intentional.
                ptstrName: windows::core::PWSTR(system_sid_buffer.as_mut_ptr().cast::<u16>()),
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            },
        },
    ];

    let mut acl_ptr = std::ptr::null_mut();
    unsafe {
        let result = SetEntriesInAclW(Some(&entries), None, &mut acl_ptr);
        if result.is_err() {
            return Err(anyhow::anyhow!("SetEntriesInAclW failed: {result:?}"));
        }
    }

    // Copy the ACL into an owned buffer so we can free the system allocation.
    let acl_buffer = unsafe {
        let acl_size = usize::from((*(acl_ptr.cast::<windows::Win32::Security::ACL>())).AclSize);
        let mut buf = vec![0u8; acl_size];
        std::ptr::copy_nonoverlapping(acl_ptr.cast::<u8>(), buf.as_mut_ptr(), acl_size);
        // windows 0.62 wraps LocalFree to take Option<HLOCAL>.
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            acl_ptr.cast::<std::ffi::c_void>(),
        )));
        buf
    };

    // --- initialise security descriptor ---
    let mut sd_buffer =
        vec![0u8; std::mem::size_of::<windows::Win32::Security::SECURITY_DESCRIPTOR>()];
    let sd_ptr = sd_buffer.as_mut_ptr().cast::<std::ffi::c_void>();

    unsafe {
        InitializeSecurityDescriptor(PSECURITY_DESCRIPTOR(sd_ptr), SECURITY_DESCRIPTOR_REVISION)
            .map_err(|e| anyhow::anyhow!("InitializeSecurityDescriptor failed: {e}"))?;

        SetSecurityDescriptorDacl(
            PSECURITY_DESCRIPTOR(sd_ptr),
            true,
            Some(acl_buffer.as_ptr().cast()),
            false,
        )
        .map_err(|e| anyhow::anyhow!("SetSecurityDescriptorDacl failed: {e}"))?;
    }

    Ok(SecurityDescriptorHolder {
        sd_ptr,
        _sd_buffer: sd_buffer,
        _acl_buffer: acl_buffer,
        _user_sid_buffer: user_sid_buffer,
        _system_sid_buffer: system_sid_buffer,
    })
}

// --------------- wire format helpers ---------------

/// Read a length-prefixed JSON message from a pipe handle.
fn read_message(pipe: windows::Win32::Foundation::HANDLE) -> Result<IpcRequest, anyhow::Error> {
    use windows::Win32::Storage::FileSystem::ReadFile;

    // 4-byte little-endian length prefix
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

    let msg_len = usize::from(u32::from_le_bytes(len_buf));
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
        total_read += usize::from(chunk_read);
    }

    let request: IpcRequest = serde_json::from_slice(&body)?;
    Ok(request)
}

/// Write a length-prefixed JSON message to a pipe handle.
fn write_message(
    pipe: windows::Win32::Foundation::HANDLE,
    response: &IpcResponse,
) -> Result<(), anyhow::Error> {
    use windows::Win32::Storage::FileSystem::WriteFile;

    let body = serde_json::to_vec(response)?;
    let len = u32::try_from(body.len())
        .expect("message body fits in u32")
        .to_le_bytes();

    let mut written: u32 = 0;
    unsafe {
        WriteFile(pipe, Some(&len), Some(&mut written), None)
            .map_err(|e| anyhow::anyhow!("failed to write length prefix: {e}"))?;
        WriteFile(pipe, Some(&body), Some(&mut written), None)
            .map_err(|e| anyhow::anyhow!("failed to write message body: {e}"))?;
    }

    Ok(())
}
