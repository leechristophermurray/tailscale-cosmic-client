//! Reproduces the app's Caddy connection flow against a real peer.
//! `cargo run -p caddy-admin --example connect -- <host>`

use caddy_admin::{CaddyAdmin, Reachability, Tunnel, probe_peer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::args().nth(1).expect("usage: connect <host>");

    println!("probing {host}:2019 directly...");
    let (client, _tunnel) = match probe_peer(&host).await {
        Reachability::Direct(endpoint) => {
            println!("  reachable directly at {endpoint}");
            (CaddyAdmin::new(endpoint), None)
        }
        Reachability::NeedsTunnel => {
            println!("  not reachable; opening a Tailscale SSH tunnel");
            let mut tunnel = Tunnel::open(&host, 12019)?;
            tunnel
                .wait_until_ready(std::time::Duration::from_secs(12))
                .await
                .map_err(|reason| format!("tunnel failed: {reason}"))?;
            let endpoint = tunnel.endpoint().clone();
            println!(
                "  tunnel up: dialing {endpoint}, claiming Host: {}",
                endpoint.origin()
            );
            (CaddyAdmin::new(endpoint), Some(tunnel))
        }
    };

    client.probe().await?;
    println!("probe OK — this is Caddy");

    let servers = client.servers().await?;
    println!("servers: {:?}", servers.keys().collect::<Vec<_>>());

    for (name, server) in &servers {
        println!(
            "  {name} listening on {:?}, {} top-level routes",
            server.listen,
            server.routes.len()
        );
        for site in server.sites() {
            let removable = if site.id.is_some() { "" } else { "  (not removable: no @id)" };
            println!("    {:<24} -> {}{removable}", site.host, site.target);
        }
    }

    Ok(())
}
