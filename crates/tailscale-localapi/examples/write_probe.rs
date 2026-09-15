//! Exercises the prefs *write* path without changing anything.
//!
//! It reads the current prefs and writes one field back to the value it already
//! has, which proves the PATCH, the masked-prefs encoding, and the CSRF header
//! all work end to end. Nothing about the machine's configuration changes.

use tailscale_localapi::{LocalApi, MaskedPrefs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = LocalApi::default();

    let before = api.prefs().await?;
    println!("before: want_running={}", before.want_running);

    // Deliberately a no-op: the same value it already holds.
    let after = api
        .set_prefs(MaskedPrefs::new().want_running(before.want_running))
        .await?;

    println!("after:  want_running={}", after.want_running);
    assert_eq!(
        before.want_running, after.want_running,
        "a no-op write must not change anything"
    );
    println!("write path OK — PATCH accepted, nothing changed");

    Ok(())
}
