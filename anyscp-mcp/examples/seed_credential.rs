//! Dev helper: store a credential the way the anySCP app does, so the MCP
//! server can be tested without the app. Usage:
//!   cargo run --example seed_credential -- <vault-key> '<json>'
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (key, json) = (&args[0], &args[1]);
    let entry = keyring::Entry::new("com.anyscp.credentials", key).expect("entry");
    entry.set_password(json).expect("set");
    let back = entry.get_password().expect("get");
    assert_eq!(&back, json);
    println!("stored {key}");
}
