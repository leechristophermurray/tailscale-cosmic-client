# Panel tooltips — what the icon means without opening the flyout.
tooltip-off = Tailscale is off
tooltip-connecting = Tailscale is connecting…
tooltip-connected = Connected to { $tailnet }
tooltip-exit-node = { $tailnet } — via exit node

# Flyout
daemon-unreachable = tailscaled is not reachable. Start it with `systemctl start tailscaled`.
suspended = Suspended — reconnecting within the hour
daemon-offline = Daemon unreachable

exit-node = EXIT NODE
exit-node-none = None (direct mesh)
exit-node-unavailable = No exit nodes advertised
peer-offline-suffix = { $name } — offline

recent-peers = RECENT PEERS
peers-loading = Loading peers…
peers-none = No peers online
peers-copy-hint = Click a peer to copy its address

open-main-window = Open Tailscale…
suspend-for-an-hour = Suspend for 1 hour
admin-console = Admin console

# Monitoring, when a hub is configured in the main window
monitoring = MONITORING
monitoring-all-well = { $count } machines healthy
monitoring-pin = pin
monitoring-unpin = unpin

# Beszel alert notifications. The hub records each rule's threshold, not the
# reading that crossed it.
alert-fired-summary = Alert on { $machine }
alert-resolved-summary = Alert cleared on { $machine }
alert-many-summary = { $count } monitoring alerts changed
alert-unknown-machine = a monitored machine
alert-status-down = { $machine } has stopped reporting to the hub
alert-status-up = { $machine } is reporting to the hub again
alert-above = { $label } is above { $threshold }
alert-below = { $label } is below { $threshold }
alert-back-below = { $label } is back below { $threshold }
alert-back-above = { $label } is back above { $threshold }
alert-label-cpu = CPU usage
alert-label-memory = Memory usage
alert-label-disk = Disk usage
alert-label-temperature = Temperature
alert-label-bandwidth = Bandwidth
alert-label-gpu = GPU usage
alert-label-load = { $minutes }-minute load average
alert-label-battery = Battery
