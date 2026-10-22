#![cfg(any(target_os = "linux", target_os = "android"))]
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use wex_access_store::{Grant, Store};
use wex_core::access::Access;
use wex_runtime::{
    wex_local_read_abi_version, wex_local_read_bytes, wex_local_read_close, wex_local_read_open,
    wex_local_read_register,
};
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-read-abi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(p.join("private/files")).unwrap();
        fs::create_dir(p.join("public")).unwrap();
        fs::write(p.join("public/file"), [0, 255, 42, 0]).unwrap();
        Self(p)
    }
    fn config(&self) -> Value {
        json!({"version":1,
            "privateRoot":self.0.join("private/files/access-grants"),
            "internalRoots":[self.0.join("private")]})
    }
    fn open(&self) -> Token {
        let bytes = serde_json::to_vec(&self.config()).unwrap();
        let mut handle = 999;
        assert_eq!(
            unsafe { wex_local_read_open(bytes.as_ptr(), bytes.len() as u32, &mut handle) },
            0
        );
        assert!(handle > 0 && handle <= i64::MAX as u64);
        Token(handle)
    }
    fn register(&self, token: &Token) -> String {
        let path = self.0.join("public").to_str().unwrap().as_bytes().to_vec();
        let mut out = [0xa5; 72];
        let mut len = 999;
        assert_eq!(
            unsafe {
                wex_local_read_register(
                    token.0,
                    path.as_ptr(),
                    path.len() as u32,
                    out.as_mut_ptr(),
                    64,
                    &mut len,
                )
            },
            0
        );
        assert_eq!(len, 64);
        assert_eq!(&out[64..], &[0xa5; 8]);
        String::from_utf8(out[..64].to_vec()).unwrap()
    }
    fn grant(&self, id: &str, personal: bool) {
        let store = Store::open(self.0.join("private/files/access-grants")).unwrap();
        store
            .set(
                store.load().unwrap().revision,
                Grant {
                    directory_id: id.into(),
                    access: Access::Read,
                    explicit_personal: personal,
                },
            )
            .unwrap();
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Token(u64);
impl Drop for Token {
    fn drop(&mut self) {
        if self.0 != 0 {
            assert_eq!(wex_local_read_close(self.0), 0);
        }
    }
}
fn read(token: u64, id: &str, leaf: &str, system: u8, cap: u32) -> (i32, u32, Vec<u8>) {
    let mut out = vec![0xa5; 65544];
    let mut len = 999;
    let code = unsafe {
        wex_local_read_bytes(
            token,
            id.as_ptr(),
            id.len() as u32,
            leaf.as_ptr(),
            leaf.len() as u32,
            system,
            out.as_mut_ptr(),
            cap,
            &mut len,
        )
    };
    (code, len, out)
}

#[test]
fn open_validates_memory_and_layout_before_store_creation() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    assert_eq!(wex_local_read_abi_version(), 1);
    let mut handle = 999;
    assert_eq!(
        unsafe { wex_local_read_open(std::ptr::null(), 1, &mut handle) },
        -1
    );
    assert_eq!(handle, 0);
    let bytes = serde_json::to_vec(&s.config()).unwrap();
    assert_eq!(
        unsafe { wex_local_read_open(bytes.as_ptr(), bytes.len() as u32, std::ptr::null_mut()) },
        -1
    );
    assert!(!s.0.join("private/files/access-grants").exists());
}

#[test]
fn strict_layout_errors_never_create_an_authority_directory() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let mut values = vec![json!({}), s.config()];
    values[1]["extra"] = json!(true);
    for key_value in [
        ("version", json!(2)),
        ("version", json!(1.0)),
        ("internalRoots", json!([])),
        ("internalRoots", json!(["relative"])),
        (
            "internalRoots",
            json!([s.0.join("private"), s.0.join("private")]),
        ),
        ("privateRoot", json!(s.0.join("private/files/other"))),
    ] {
        let mut v = s.config();
        v[key_value.0] = key_value.1;
        values.push(v);
    }
    for value in values {
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut handle = 999;
        assert_eq!(
            unsafe { wex_local_read_open(bytes.as_ptr(), bytes.len() as u32, &mut handle) },
            -4
        );
        assert_eq!(handle, 0);
        assert!(!s.0.join("private/files/access-grants").exists());
    }
    let valid = serde_json::to_string(&s.config()).unwrap();
    let duplicate = valid.replacen('{', "{\"version\":1,", 1).into_bytes();
    for bytes in [vec![0xff], duplicate] {
        let mut handle = 999;
        assert_eq!(
            unsafe { wex_local_read_open(bytes.as_ptr(), bytes.len() as u32, &mut handle) },
            -4
        );
        assert_eq!(handle, 0);
        assert!(!s.0.join("private/files/access-grants").exists());
    }
}

#[test]
fn handles_are_bounded_closed_and_never_reused() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let mut tokens: Vec<_> = (0..16).map(|_| s.open()).collect();
    let bytes = serde_json::to_vec(&s.config()).unwrap();
    let mut handle = 999;
    assert_eq!(
        unsafe { wex_local_read_open(bytes.as_ptr(), bytes.len() as u32, &mut handle) },
        -7
    );
    assert_eq!(handle, 0);
    let mut old = tokens.pop().unwrap();
    assert_eq!(wex_local_read_close(old.0), 0);
    assert_eq!(wex_local_read_close(old.0), -8);
    let next = s.open();
    assert!(next.0 > old.0);
    assert_eq!(read(old.0, &"a".repeat(64), "file", 1, 65536).0, -8);
    old.0 = 0;
}

#[test]
fn live_grants_system_state_and_revocation_control_binary_read() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let token = s.open();
    let id = s.register(&token);
    assert_eq!(read(token.0, &id, "file", 1, 65536).0, -10);
    s.grant(&id, false);
    assert_eq!(read(token.0, &id, "file", 1, 65536).0, -11);
    s.grant(&id, true);
    assert_eq!(read(token.0, &id, "file", 0, 65536).0, -12);
    let (code, len, out) = read(token.0, &id, "file", 1, 65536);
    assert_eq!((code, len), (0, 4));
    assert_eq!(&out[..4], &[0, 255, 42, 0]);
    assert!(out[4..].iter().all(|b| *b == 0xa5));
    let admin = Store::open(s.0.join("private/files/access-grants")).unwrap();
    admin.revoke(admin.load().unwrap().revision, &id).unwrap();
    let (code, len, out) = read(token.0, &id, "file", 1, 65536);
    assert_eq!((code, len), (-10, 0));
    assert!(out.iter().all(|b| *b == 0xa5));
}

#[test]
fn short_buffers_reject_before_registration_or_content_io() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let token = s.open();
    let path = s.0.join("public").to_str().unwrap().as_bytes().to_vec();
    let mut id = [0xa5; 64];
    let mut len = 999;
    assert_eq!(
        unsafe {
            wex_local_read_register(
                token.0,
                path.as_ptr(),
                path.len() as u32,
                id.as_mut_ptr(),
                63,
                &mut len,
            )
        },
        -2
    );
    assert_eq!(len, 0);
    assert_eq!(id, [0xa5; 64]);
    let id = s.register(&token);
    s.grant(&id, true);
    let (code, len, out) = read(token.0, &id, "file", 1, 65535);
    assert_eq!((code, len), (-2, 0));
    assert!(out.iter().all(|b| *b == 0xa5));
    assert_eq!(read(token.0, &id, "file", 2, 65536).0, -1);
    let (code, len, out) = read(token.0, &id, "", 1, 65536);
    assert_eq!((code, len), (-1, 0));
    assert!(out.iter().all(|b| *b == 0xa5));
    let mut len = 999;
    assert_eq!(
        unsafe {
            wex_local_read_bytes(
                token.0,
                id.as_ptr(),
                64,
                b"file".as_ptr(),
                4,
                1,
                std::ptr::null_mut(),
                65536,
                &mut len,
            )
        },
        -1
    );
    assert_eq!(len, 0);
    assert_eq!(read(token.0, &id, "file", 1, 65536).0, 0);
}

#[test]
fn internal_directories_cannot_be_registered_through_ffi() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let token = s.open();
    let path = s.0.join("private").to_str().unwrap().as_bytes().to_vec();
    let mut id = [0xa5; 64];
    let mut len = 999;
    assert_eq!(
        unsafe {
            wex_local_read_register(
                token.0,
                path.as_ptr(),
                path.len() as u32,
                id.as_mut_ptr(),
                64,
                &mut len,
            )
        },
        -9
    );
    assert_eq!(len, 0);
    assert_eq!(id, [0xa5; 64]);
    let id = s.register(&token);
    s.grant(&id, true);
    assert_eq!(read(token.0, &id, "file", 1, 65536).0, 0);
}

#[test]
fn file_limits_zero_length_and_new_session_identity_are_preserved() {
    let _serial = SERIAL.lock().unwrap();
    let s = Sandbox::new();
    let token = s.open();
    let id = s.register(&token);
    s.grant(&id, true);
    fs::write(s.0.join("public/empty"), []).unwrap();
    fs::write(s.0.join("public/exact"), vec![7; 65536]).unwrap();
    fs::write(s.0.join("public/large"), vec![7; 65537]).unwrap();
    assert_eq!(read(token.0, &id, "empty", 1, 65536).0, 0);
    assert_eq!(read(token.0, &id, "empty", 1, 65536).1, 0);
    assert_eq!(read(token.0, &id, "exact", 1, 65536).1, 65536);
    let (code, len, out) = read(token.0, &id, "large", 1, 65536);
    assert_eq!((code, len), (-18, 0));
    assert!(out.iter().all(|b| *b == 0xa5));
    assert_eq!(read(token.0, &id, "../file", 1, 65536).0, -1);
    let next = s.open();
    let next_id = s.register(&next);
    assert_ne!(id, next_id);
    assert_eq!(read(next.0, &id, "file", 1, 65536).0, -17);
    assert_eq!(read(next.0, &next_id, "file", 1, 65536).0, -10);
}
