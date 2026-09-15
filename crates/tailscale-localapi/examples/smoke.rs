//! Read-only smoke test against the local tailscaled.
//! `cargo run -p tailscale-localapi --example smoke`

use futures_util::StreamExt;
use tailscale_localapi::{LocalApi, PingType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = LocalApi::default();

    let status = api.status().await?;
    println!(
        "daemon      {} ({:?})",
        status.version, status.backend_state
    );
    println!("tailnet     {}", status.tailnet_name());
    if let Some(me) = &status.self_status {
        println!(
            "this node   {} {} expiry={:?}d",
            me.display_name(),
            me.ipv4().unwrap_or("-"),
            me.days_until_key_expiry()
        );
    }

    println!("\npeers ({})", status.peer.len());
    for peer in status.peers_sorted() {
        println!(
            "  {:<20} {:<16} {:<8} {:?} ssh={} taildrop={}",
            peer.display_name(),
            peer.ipv4().unwrap_or("-"),
            if peer.online { "online" } else { "offline" },
            peer.route(),
            peer.supports_ssh(),
            peer.can_receive_files(),
        );
    }

    let prefs = api.prefs().await?;
    println!(
        "\nprefs       want_running={} exit_node={:?} ssh={} shields={} routes={:?}",
        prefs.want_running,
        prefs.exit_node_id,
        prefs.run_ssh,
        prefs.shields_up,
        prefs.subnet_routes()
    );
    if let Some(profile) = prefs.user_profile() {
        println!(
            "account     {} [{}]",
            profile.login_name,
            profile.initials()
        );
    }

    println!(
        "\nexit node options: {:?}",
        status
            .exit_node_options()
            .iter()
            .map(|p| p.display_name())
            .collect::<Vec<_>>()
    );

    let targets = api.file_targets().await?;
    println!(
        "taildrop targets: {:?}",
        targets.iter().map(FileTargetName::name).collect::<Vec<_>>()
    );

    let serve = api.serve_config().await?;
    println!("serve entries: {}", serve.entries().len());
    for entry in serve.entries() {
        println!(
            "  {} -> {} ({})",
            entry.url(),
            entry.target,
            entry.scope.label()
        );
    }

    if let Some((peer, ip)) = status
        .peers_sorted()
        .iter()
        .find(|p| p.online)
        .and_then(|peer| Some((*peer, peer.ipv4()?)))
    {
        let ping = api.ping(ip, PingType::Disco).await?;
        println!("\nping {} => {}", peer.display_name(), ping.summary());
    }

    println!("\nwatching IPN bus for 3 frames...");
    let mut stream = Box::pin(api.watch().await?);
    let mut seen = 0;
    while let Some(notify) = stream.next().await {
        let notify = notify?;
        if !notify.is_meaningful() {
            continue;
        }
        println!(
            "  frame state={:?} prefs={} engine={:?}",
            notify.state,
            notify.prefs.is_some(),
            notify.engine.map(|e| (e.rx_bytes, e.tx_bytes, e.num_live))
        );
        seen += 1;
        if seen >= 3 {
            break;
        }
    }

    Ok(())
}

trait FileTargetName {
    fn name(&self) -> &str;
}
impl FileTargetName for tailscale_localapi::FileTarget {
    fn name(&self) -> &str {
        self.display_name()
    }
}
