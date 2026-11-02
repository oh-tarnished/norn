//! Owner-only security descriptors for named pipes. **The only `unsafe` in
//! this crate.**
//!
//! # Why `unsafe` (SPEC §1)
//!
//! A pipe created without a descriptor gets the default DACL, which grants
//! LocalSystem and Administrators full control and Everyone read access.
//! SPEC §41 asks for the current user only. Neither std nor tokio can build
//! a descriptor, and tokio accepts one only through the `unsafe`
//! `create_with_security_attributes_raw`. Finding the current user's SID
//! needs the Win32 token API. No pure-Rust crate does this without the same
//! FFI, so it is done here, once, with every call checked and every
//! allocation released by a guard.
//!
//! The descriptor is SDDL `D:P(A;;GA;;;<user SID>)`: a protected DACL
//! (nothing inherited) with one ACE granting generic-all to the user who
//! started the daemon. Every other principal, including Administrators, is
//! denied by absence.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// A SID string is at most ~184 UTF-16 units; anything longer is corrupt.
const MAX_SID_CHARS: usize = 256;

/// The owner-only SDDL, held as a NUL-terminated UTF-16 string.
///
/// Plain data, so `Send + Sync`; the OS descriptor is materialised per pipe
/// instance inside [`OwnerOnly::create`] and freed before it returns, so no
/// raw pointer is ever held across an `.await`.
pub(super) struct OwnerOnly {
    sddl: Vec<u16>,
}

impl OwnerOnly {
    /// Builds the descriptor text for the user running this process.
    pub(super) fn for_current_user() -> io::Result<Self> {
        let sid = current_user_sid()?;
        let text = format!("D:P(A;;GA;;;{sid})");
        Ok(Self {
            sddl: text.encode_utf16().chain(Some(0)).collect(),
        })
    }

    /// Creates a pipe instance named `name` from `options`, protected by
    /// this descriptor.
    pub(super) fn create(
        &self,
        options: &ServerOptions,
        name: &str,
    ) -> io::Result<NamedPipeServer> {
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `sddl` is NUL-terminated UTF-16; `descriptor` is a valid
        // out-pointer; the size out-pointer may be null per the Win32 docs.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                self.sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let descriptor = LocalBox(descriptor);
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // SAFETY: `attributes` is a fully initialised SECURITY_ATTRIBUTES
        // whose descriptor (owned by `descriptor`) outlives this call; the
        // OS copies it into the pipe object, keeping no reference to ours.
        unsafe {
            options
                .create_with_security_attributes_raw(name, (&raw mut attributes).cast::<c_void>())
        }
    }
}

/// Frees a `LocalAlloc`ed pointer on drop.
struct LocalBox(*mut c_void);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Win32 call documented to
            // allocate with LocalAlloc, and is freed exactly once, here.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// Closes a kernel handle on drop.
struct HandleBox(HANDLE);

impl Drop for HandleBox {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The current process user's SID as a string, e.g. `S-1-5-21-…-1001`.
fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no
    // closing; `token` is a valid out-pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = HandleBox(token);
    let mut needed = 0u32;
    // SAFETY: a null buffer of length 0 is the documented size query; it
    // fails with ERROR_INSUFFICIENT_BUFFER and writes the size to `needed`.
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    let bytes = needed as usize;
    if bytes < size_of::<TOKEN_USER>() {
        return Err(io::Error::last_os_error());
    }
    // u64 words: TOKEN_USER holds a pointer, so it needs pointer alignment.
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    // SAFETY: `buffer` is writable for at least `needed` bytes.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the OS filled `buffer` with a TOKEN_USER (size checked above,
    // alignment guaranteed by u64); its SID points inside `buffer`, which
    // outlives every use below.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut wide: *mut u16 = null_mut();
    // SAFETY: the SID is valid (above); `wide` is a valid out-pointer.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut wide) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let owned = LocalBox(wide.cast());
    let mut len = 0;
    // SAFETY: the OS returned a NUL-terminated string; the scan stops at the
    // terminator or at MAX_SID_CHARS, never reading past either.
    while len < MAX_SID_CHARS && unsafe { *wide.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` units starting at `wide` were just read successfully.
    let units = unsafe { std::slice::from_raw_parts(wide, len) };
    let sid = String::from_utf16(units).map_err(io::Error::other)?;
    drop(owned);
    Ok(sid)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_grants_only_the_current_user() {
        let owner = OwnerOnly::for_current_user().unwrap();
        let text = String::from_utf16(&owner.sddl[..owner.sddl.len() - 1]).unwrap();
        assert!(text.starts_with("D:P(A;;GA;;;S-1-"), "{text}");
        assert_eq!(text.matches("(A;").count(), 1, "{text}");
    }

    #[tokio::test]
    async fn a_pipe_with_the_descriptor_accepts_its_own_user() {
        let name = format!(r"\\.\pipe\norn-security-test-{}", std::process::id());
        let owner = OwnerOnly::for_current_user().unwrap();
        let mut options = ServerOptions::new();
        options.first_pipe_instance(true);
        let server = owner.create(&options, &name).unwrap();
        let client = tokio::net::windows::named_pipe::ClientOptions::new().open(&name);
        assert!(client.is_ok(), "{client:?}");
        server.connect().await.unwrap();
    }
}
