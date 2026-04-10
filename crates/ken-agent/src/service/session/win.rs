//! Windows implementation of tray app session management.
//!
//! Uses `WTSEnumerateSessions`, `WTSQueryUserToken`,
//! `CreateEnvironmentBlock`, and `CreateProcessAsUser` to launch
//! `ken-agent.exe tray` in interactive user sessions.

use std::sync::Arc;

use ken_protocol::audit::{AuditEventKind, TrayLaunchTrigger, TrayTerminationReason};

use crate::audit::AuditLogger;
use crate::service::session::{TrayProcessInfo, TrayProcessMap};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{
    DuplicateTokenEx, SecurityImpersonation, TokenPrimary, TOKEN_ALL_ACCESS,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::RemoteDesktop::{
    WTSActive, WTSEnumerateSessionsW, WTSFreeMemory, WTSQueryUserToken, WTS_CURRENT_SERVER_HANDLE,
    WTS_SESSION_INFOW,
};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW,
    CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
};

/// Enumerate currently active interactive sessions.
///
/// Returns a list of session IDs that have a logged-in user. Used at
/// service startup to launch tray apps for sessions that already exist
/// (the typical reboot path where the user logs in before the service
/// starts, or vice versa).
pub fn enumerate_active_sessions() -> Vec<u32> {
    let mut session_info_ptr: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut count: u32 = 0;

    let result = unsafe {
        WTSEnumerateSessionsW(
            WTS_CURRENT_SERVER_HANDLE,
            0,
            1,
            &raw mut session_info_ptr,
            &raw mut count,
        )
    };

    if result.is_err() {
        tracing::warn!("WTSEnumerateSessionsW failed");
        return Vec::new();
    }

    let mut active_sessions = Vec::new();

    if !session_info_ptr.is_null() && count > 0 {
        let sessions = unsafe { std::slice::from_raw_parts(session_info_ptr, count as usize) };

        for session in sessions {
            // Only consider active sessions (user is logged in and
            // the session is connected).
            if session.State == WTSActive && session.SessionId != 0 {
                active_sessions.push(session.SessionId);
            }
        }

        unsafe {
            WTSFreeMemory(session_info_ptr.cast());
        }
    }

    active_sessions
}

/// Launch `ken-agent.exe tray` in the given session.
///
/// Resolves the user token via `WTSQueryUserToken`, creates an
/// environment block, and calls `CreateProcessAsUser`. Returns the
/// process info on success, or an error string on failure.
///
/// Matches the Windows API conventions in `crates/ken-agent/src/ipc/server.rs`
/// for handle management and error wrapping.
pub fn launch_tray_in_session(session_id: u32) -> Result<TrayProcessInfo, String> {
    // --- Step 1: Get user token for the session ---
    let mut user_token = HANDLE::default();
    unsafe {
        WTSQueryUserToken(session_id, &raw mut user_token)
            .map_err(|e| format!("WTSQueryUserToken failed for session {session_id}: {e}"))?;
    }

    // --- Step 2: Duplicate to a primary token ---
    // CreateProcessAsUser requires a primary token. WTSQueryUserToken
    // returns an impersonation token on some Windows versions, so we
    // duplicate it to ensure we have a primary token.
    let mut primary_token = HANDLE::default();
    let dup_result = unsafe {
        DuplicateTokenEx(
            user_token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &raw mut primary_token,
        )
    };

    // Close the original token regardless of dup result.
    unsafe {
        let _ = CloseHandle(user_token);
    }

    dup_result.map_err(|e| format!("DuplicateTokenEx failed for session {session_id}: {e}"))?;

    // --- Step 3: Create environment block for the user ---
    let mut env_block: *mut std::ffi::c_void = std::ptr::null_mut();
    let env_result = unsafe { CreateEnvironmentBlock(&raw mut env_block, primary_token, false) };

    if env_result.is_err() {
        unsafe {
            let _ = CloseHandle(primary_token);
        }
        return Err(format!(
            "CreateEnvironmentBlock failed for session {session_id}"
        ));
    }

    // --- Step 4: Build the command line ---
    let exe_path =
        std::env::current_exe().map_err(|e| format!("failed to get current exe path: {e}"))?;
    let command_line = format!("\"{}\" tray", exe_path.display());
    let mut command_line_wide: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // --- Step 5: Set up STARTUPINFO ---
    // Target the interactive desktop so the tray app can display its UI.
    let desktop = "winsta0\\default";
    let mut desktop_wide: Vec<u16> = desktop.encode_utf16().chain(std::iter::once(0)).collect();

    let mut startup_info = STARTUPINFOW {
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).expect("STARTUPINFOW size fits u32"),
        lpDesktop: PWSTR(desktop_wide.as_mut_ptr()),
        ..Default::default()
    };

    let mut process_info = PROCESS_INFORMATION::default();

    // --- Step 6: CreateProcessAsUser ---
    let create_result = unsafe {
        CreateProcessAsUserW(
            primary_token,
            None, // application name — derived from command line
            PWSTR(command_line_wide.as_mut_ptr()),
            None, // process security attributes
            None, // thread security attributes
            false,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            Some(env_block),
            None, // current directory — inherit
            &raw const startup_info,
            &raw mut process_info,
        )
    };

    // Clean up environment block and token regardless of result.
    unsafe {
        let _ = DestroyEnvironmentBlock(env_block);
        let _ = CloseHandle(primary_token);
    }

    create_result
        .map_err(|e| format!("CreateProcessAsUser failed for session {session_id}: {e}"))?;

    // Close the thread handle — we only need the process handle.
    unsafe {
        let _ = CloseHandle(process_info.hThread);
    }

    let pid = process_info.dwProcessId;
    tracing::info!(session_id, pid, "launched tray app in session");

    Ok(TrayProcessInfo {
        session_id,
        process_handle: process_info.hProcess,
        pid,
    })
}

/// Terminate a tracked tray app process.
///
/// Per the logoff handling decision in the task prompt: there is no
/// graceful IPC shutdown message yet (no `Shutdown` variant in the IPC
/// protocol). We terminate the process directly and close the handle.
///
// TODO: When a graceful shutdown IPC message is added per ADR-0010,
// send it first, wait up to 2 seconds for exit, then fall back to
// TerminateProcess. See Issue #10 prompt and ADR-0010.
pub fn terminate_tray_process(info: &TrayProcessInfo) {
    // Check if the process has already exited (crash, user killed it).
    let wait_result = unsafe { WaitForSingleObject(info.process_handle, 0) };
    if wait_result == WAIT_OBJECT_0 {
        tracing::debug!(
            session_id = info.session_id,
            pid = info.pid,
            "tray process already exited"
        );
        unsafe {
            let _ = CloseHandle(info.process_handle);
        }
        return;
    }

    // Process is still alive — terminate it.
    let term_result = unsafe { TerminateProcess(info.process_handle, 0) };
    if let Err(e) = term_result {
        tracing::warn!(
            session_id = info.session_id,
            pid = info.pid,
            error = %e,
            "TerminateProcess failed"
        );
    } else {
        tracing::info!(
            session_id = info.session_id,
            pid = info.pid,
            "terminated tray process"
        );
    }

    unsafe {
        let _ = CloseHandle(info.process_handle);
    }
}

/// Handle a session logon event: launch a tray app in the new session.
///
/// If the map already contains an entry for this session ID (defensive
/// against duplicate events), the old process is terminated first.
pub fn handle_session_logon(
    session_id: u32,
    trigger: TrayLaunchTrigger,
    map: &mut TrayProcessMap,
    audit: &Arc<AuditLogger>,
) {
    // Defensive: if we already have a tray process for this session,
    // terminate it first (duplicate SessionLogon events).
    if let Some(old) = map.remove(&session_id) {
        tracing::warn!(
            session_id,
            old_pid = old.pid,
            "duplicate logon for session, terminating old tray process"
        );
        terminate_tray_process(&old);
        audit.log(
            AuditEventKind::TrayTerminated {
                session_id,
                reason: TrayTerminationReason::SessionLogoff,
            },
            &format!(
                "terminated stale tray process (pid {}) for session {session_id} before re-launch",
                old.pid
            ),
        );
    }

    match launch_tray_in_session(session_id) {
        Ok(info) => {
            audit.log(
                AuditEventKind::TrayLaunched {
                    session_id,
                    trigger: trigger.clone(),
                },
                &format!(
                    "launched tray app (pid {}) in session {session_id} (trigger: {trigger:?})",
                    info.pid
                ),
            );
            map.insert(session_id, info);
        }
        Err(e) => {
            audit.log(
                AuditEventKind::TrayLaunchFailed {
                    session_id,
                    error: e.clone(),
                },
                &format!("failed to launch tray app in session {session_id}: {e}"),
            );
        }
    }
}

/// Handle a session logoff event: terminate the tray app for the session.
pub fn handle_session_logoff(session_id: u32, map: &mut TrayProcessMap, audit: &Arc<AuditLogger>) {
    if let Some(info) = map.remove(&session_id) {
        terminate_tray_process(&info);
        audit.log(
            AuditEventKind::TrayTerminated {
                session_id,
                reason: TrayTerminationReason::SessionLogoff,
            },
            &format!(
                "terminated tray app (pid {}) in session {session_id} on logoff",
                info.pid
            ),
        );
    } else {
        tracing::debug!(
            session_id,
            "logoff for session with no tracked tray process"
        );
    }
}

/// Terminate all tracked tray processes on service shutdown.
///
/// Called during the clean shutdown sequence, before reporting
/// `ServiceState::Stopped`.
pub fn terminate_all_tray_processes(map: &mut TrayProcessMap, audit: &Arc<AuditLogger>) {
    let session_ids: Vec<u32> = map.keys().copied().collect();
    for session_id in session_ids {
        if let Some(info) = map.remove(&session_id) {
            terminate_tray_process(&info);
            audit.log(
                AuditEventKind::TrayTerminated {
                    session_id,
                    reason: TrayTerminationReason::ServiceShutdown,
                },
                &format!(
                    "terminated tray app (pid {}) in session {session_id} on service shutdown",
                    info.pid
                ),
            );
        }
    }
}

/// Launch tray apps in all currently active interactive sessions.
///
/// Called at service startup to handle the case where the service
/// starts after a user is already logged in (the typical reboot path).
pub fn launch_for_active_sessions(map: &mut TrayProcessMap, audit: &Arc<AuditLogger>) {
    let sessions = enumerate_active_sessions();
    if sessions.is_empty() {
        tracing::info!("no active interactive sessions at startup");
        return;
    }

    tracing::info!(
        count = sessions.len(),
        "found active interactive sessions at startup"
    );
    for session_id in sessions {
        handle_session_logon(session_id, TrayLaunchTrigger::Startup, map, audit);
    }
}
