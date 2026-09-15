//! Check this crate against a real Beszel hub.
//!
//! Credentials come from the environment so they never appear in a command
//! line, shell history, or anyone else's terminal:
//!
//! ```sh
//! BESZEL_URL=https://mon.example.com \
//! BESZEL_USER=you@example.com \
//! BESZEL_PASSWORD=... \
//!   cargo run -p beszel-client --example probe
//! ```

use beszel_client::{BeszelHub, StatsPeriod};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("BESZEL_URL").expect("set BESZEL_URL");
    let user = std::env::var("BESZEL_USER").expect("set BESZEL_USER");
    let password = std::env::var("BESZEL_PASSWORD").expect("set BESZEL_PASSWORD");

    let mut hub = BeszelHub::new(&url)?;

    hub.health().await?;
    println!("hub at {url} is healthy");

    hub.authenticate(&user, &password).await?;
    println!("signed in");

    match hub.info().await {
        Ok(info) => println!(
            "hub version {} · public key {}…",
            info.version,
            info.key.chars().take(24).collect::<String>()
        ),
        Err(error) => println!("could not read hub info: {error}"),
    }

    let systems = hub.systems().await?;
    println!("\n{} systems:", systems.len());

    for system in &systems {
        println!(
            "  {:<18} {:<10} cpu {:>5.1}%  mem {:>5.1}%  disk {:>5.1}%  up {}  agent {}",
            system.name,
            system.status.label(),
            system.info.cpu,
            system.info.memory_pct,
            system.info.disk_pct,
            system.info.uptime_human(),
            system.info.agent_version,
        );

        if let Some(failed) = system.info.failed_services().filter(|n| *n > 0) {
            println!("      {failed} failed systemd service(s)");
        }
    }

    // Look at the first reporting machine in detail.
    let Some(system) = systems.iter().find(|s| s.status.is_up()) else {
        println!("\nno system is reporting, so there are no stats to read");
        return Ok(());
    };

    println!("\ndetail for {}:", system.name);

    match hub.latest_stats(&system.id).await? {
        Some(stats) => {
            println!(
                "  cpu {:.1}%  mem {:.1}/{:.1} GiB used (+{:.1} reclaimable)  disk {:.1}/{:.1} GiB",
                stats.cpu,
                stats.memory_used,
                stats.memory_total,
                stats.memory_reclaimable(),
                stats.disk_used,
                stats.disk_total,
            );
            println!("  load {:?}", stats.load_average);

            if let Some((sensor, celsius)) = stats.peak_temperature() {
                println!("  hottest sensor: {sensor} at {celsius:.1} C");
            }
            if !stats.temperatures.is_empty() {
                println!("  sensors: {:?}", stats.temperatures);
            }
            if !stats.zfs_pools.is_empty() {
                for (name, pool) in &stats.zfs_pools {
                    println!(
                        "  zfs {name}: {:.0}/{:.0} GiB ({:.1}%) {}",
                        pool.used,
                        pool.total,
                        pool.used_pct(),
                        pool.health
                    );
                }
            }
            for (pool, health) in stats.degraded_pools() {
                println!("  WARNING: pool {pool} is {health}");
            }
        }
        None => println!("  no stats recorded yet"),
    }

    let containers = hub.containers(&system.id).await?;
    println!("\n  {} containers (busiest first):", containers.len());
    for container in containers.iter().take(10) {
        println!(
            "    {:<28} cpu {:>5.1}%  mem {:>7.1} MiB{}",
            container.name,
            container.cpu,
            container.memory,
            if container.update_available {
                "  (update available)"
            } else {
                ""
            }
        );
    }

    let series = hub.stats(&system.id, StatsPeriod::TenMinutes, 6).await?;
    println!("\n  {} recent 10m samples", series.len());

    Ok(())
}
