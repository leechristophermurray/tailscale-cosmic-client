## Navigation

page-machines = Machines
page-exit-nodes = Exit Nodes
page-services = Services & DNS
page-caddy = Caddy
page-access = Access Controls
page-preferences = Preferences

## Shared

dismiss = Dismiss
admin-console = Admin console
reauthenticate = Reauthenticate
refresh-now = Refresh now
cancel = Cancel
remove = Remove
copied = Copied { $value }
not-connected-yet = Waiting for tailscaled…

## Connection header

connection-daemon-unreachable = Daemon unreachable
peers-online = { $online } of { $total } peers online
peers-active = { $count } active
exit-node-label = Exit node
exit-node-none = None (direct mesh)
exit-node-unavailable = Exit node: none advertised
peer-offline-suffix = { $name } — offline
recent-peers-heading = Recent peers & transfer targets
suspend-for-an-hour = Suspend for 1h
suspended = Suspended
drop-zone-title = Taildrop drop zone
drop-zone-detail = Drop files here to pick a machine
drop-zone-ready = { $count } ready
drop-zone-choose = Choose a machine below to send
files-one = 1 file
files-many = { $count } files
files-ready = { $files } ready — choose a machine to send to

## Machines page

this-machine = This machine
my-devices = My devices
shared-with-me = Shared with me
filter-machines = Filter { $count } machines…
no-machines-match = No machines match “{ $query }”
localapi-daemon = LocalAPI daemon
daemon-version = tailscaled { $version }
daemon-connecting = connecting…
daemon-unreachable-short = unreachable

machine-none-selected = No machine selected
machine-none-selected-detail = Pick a machine from the list to see its addresses, connection path, and actions.

mesh-quality = Mesh quality
route-direct = Direct
route-relayed = Relayed
route-relay = Relay { $region }
route-derp = DERP { $region }
route-idle = Idle
route-offline = Offline
route-online = Online
route-unreachable = Unreachable
route-no-path = no active path
route-home-relay = home relay { $region }
route-no-home-relay = no home relay assigned

action-ssh = SSH terminal
action-ping = Ping / path
action-pinging = Pinging…
action-send-file = Send file (Taildrop)
action-send-queued = Send queued files
action-send-to = Send to { $name }

stat-os = OS & platform
stat-joined = Joined { $date }
stat-endpoint = Peer endpoint
stat-direct-wireguard = Direct WireGuard
stat-relayed-no-direct = Relayed — no direct path yet
stat-no-path = No active path
stat-transfer = ↓ { $rx }  ↑ { $tx }
stat-machine-key = Machine key
key-does-not-expire = Does not expire
key-expiry-disabled = Key expiry is disabled for this node
key-valid-until = Valid until { $date }

cap-ssh-on = Tailscale SSH enabled
cap-ssh-on-detail = Reachable with tailnet identity — no keys to manage
cap-ssh-off = Tailscale SSH off
cap-ssh-off-detail = This machine does not accept Tailscale SSH
cap-exit-active = Carrying your traffic
cap-exit-active-detail = All internet traffic is routed through this node
cap-exit-available = Available as exit node
cap-exit-available-detail = Can route your internet traffic when selected
cap-exit-none = Not an exit node
cap-exit-none-detail = This machine does not advertise a default route

taildrop-queued = Queued for Taildrop
taildrop-cannot-receive = This machine cannot receive files right now.
taildrop-sending = Sending to { $name }…
taildrop-sent = Sent { $files } to { $name }
taildrop-saved = Saved { $files } to { $path }

## Exit nodes page

routing = Routing
exit-node-direct-mesh = Direct mesh
exit-node-direct-mesh-detail = Internet traffic uses this machine's own connection
exit-node-direct-mesh-row = Use this machine's own internet connection
exit-node-carrying = All internet traffic leaves through this node · { $os }
exit-nodes-available = Available exit nodes
exit-nodes-none = No machine on this tailnet advertises itself as an exit node. Run `tailscale up --advertise-exit-node` on a machine and approve the route in the admin console, or turn on the switch below to offer this one.
exit-node-active = Active
exit-node-available = Available

allow-lan-access = Allow local network access
allow-lan-access-detail = Keep printers and NAS on your LAN reachable while the exit node carries internet traffic

run-as-exit-node = Run as exit node
advertise-exit-node = Offer this machine as an exit node
advertise-exit-node-detail = Other machines on your tailnet can route their internet traffic through this one
advertise-approved = Approved — peers can select this machine
advertise-pending = Advertised, waiting for admin approval in the admin console

## Services page

magic-dns = MagicDNS
magic-dns-on = Enabled for this tailnet
magic-dns-off = Disabled for this tailnet
resolves-as = This machine resolves as
use-tailnet-dns = Use tailnet DNS settings
use-tailnet-dns-detail = Resolve MagicDNS names and use the nameservers your tailnet provides

serve-and-funnel = Serve & funnel
serve-published = { $count } published
serve-none = Nothing is published from this machine. `tailscale serve` shares a local port with your tailnet; `tailscale funnel` puts it on the public internet.
scope-tailnet = Tailnet only
scope-funnel = Public funnel

taildrop = Taildrop
taildrop-none-waiting = No files are waiting. Files sent to this machine appear here and in your downloads folder.
taildrop-targets = { $count } machines on this tailnet can receive files from here

## Access page

machine-key = Machine key
key-expired = Expired
key-renew-soon = Renew soon
key-valid = Valid
key-no-expiry = No expiry
key-unknown = Unknown
key-expiry-explain = This machine's node key is valid until { $date }. Reauthenticate before then to stay on the tailnet.
key-expiry-off-explain = Key expiry is turned off for this node, so it will not be signed out automatically.
key-expiry-waiting = Waiting for the daemon to report key state.

inbound-access = Inbound access
accept-ssh = Accept Tailscale SSH
accept-ssh-detail = Let permitted tailnet users open a shell here using their tailnet identity, with no SSH keys to distribute
shields-up = Shields up
shields-up-detail = Block all incoming connections from the tailnet. Outgoing connections and Taildrop sends still work

advertised-routes = Advertised routes
advertised-routes-none = This machine does not advertise any subnet routes.
advertised-routes-note = Advertised routes must be approved by a tailnet admin before peers can use them.

## Preferences page

accept-routes = Accept subnet routes
accept-routes-detail = Use routes that other machines advertise, so their local networks are reachable from here

account = Account
not-signed-in = Not signed in
sign-in = Sign in
tailnet-named = Tailnet { $name }

daemon = Daemon
daemon-cannot-reach = Cannot reach tailscaled. Check that the service is running with `systemctl status tailscaled`.
daemon-connecting-long = Connecting to tailscaled…
daemon-summary = tailscaled { $version } · { $state }
daemon-transfer = { $rx } received · { $tx } sent since the daemon started
daemon-warnings = Daemon warnings

## Caddy page

caddy-server = Caddy server
caddy-no-candidates = No machine on this tailnet is both online and reachable over Tailscale SSH, so there is nothing to configure from here.
caddy-machine = Machine
caddy-connect = Connect
caddy-connecting = Connecting…
caddy-not-connected = Not connected
caddy-not-connected-detail = Pick a machine and connect to read its Caddy configuration.
caddy-probing = Probing the admin API, then falling back to a Tailscale SSH tunnel.
caddy-connected-direct = Connected directly
caddy-connected-direct-detail = Admin API at { $endpoint } over the tailnet
caddy-connected-tunnel = Connected through Tailscale SSH
caddy-connected-tunnel-detail = Admin API forwarded to { $endpoint }; the port stays bound to this machine's loopback
caddy-unreachable = Not reachable

caddy-routes = Forwarded routes & virtual hosts
caddy-route-count = { $count } routes
caddy-no-routes = Caddy is running but has no HTTP routes configured.
caddy-route-unnamed = route { $index }
caddy-automatic-https = Automatic HTTPS
caddy-from-caddyfile = from Caddyfile

caddy-add-route = Add reverse proxy route
caddy-hostname = Hostname
caddy-forward-to = Forward to
caddy-https-note = A hostname ending in .ts.net gets an HTTPS certificate from Tailscale automatically — no Let's Encrypt account and no DNS challenge.
caddy-add = Add route
caddy-route-added = Added { $host } → { $upstream }
caddy-route-removed = Route removed

## Status bar and notices

status-tailscale-version = Tailscale { $version }
status-unknown-version = tailscaled —
status-throughput = In { $rx }   Out { $tx }
status-relays-one = 1 relay
status-relays-many = { $count } relays

notice-suspended = Tailscale suspended. It will reconnect in an hour.
notice-finish-signin = Finish signing in in your browser
notice-opening-ssh = Opening SSH session to { $host }
notice-drop-first = Drop files onto the window first, then pick a machine.
notice-drop-no-files = That drop carried no local files — Taildrop can only send files from this machine.
drop-zone-active = Release to queue these files
choose-files-title = Choose files to send
choose-files-accept = Send
drop-zone-choose-files = Choose files…
drop-zone-or-drop = Pick files to send to a machine

## Backend states

state-connected = Connected
state-connecting = Connecting
state-disconnected = Disconnected
state-needs-login = Sign in required
state-needs-approval = Awaiting approval
state-in-use = In use by another user
state-unknown = Unknown

state-connected-detail = Connected to your tailnet
state-connecting-detail = Bringing the tunnel up
state-disconnected-detail = Tailscale is off
state-needs-login-detail = Sign in to join your tailnet
state-needs-approval-detail = Waiting for an admin to approve this machine
state-in-use-detail = Another user on this machine holds the daemon
state-no-state-detail = The daemon has not reported a state yet
state-unknown-detail = Cannot reach tailscaled

## Operating systems

os-linux = Linux
os-macos = macOS
os-windows = Windows
os-ios = iOS
os-android = Android
os-freebsd = FreeBSD
os-openbsd = OpenBSD
os-tvos = tvOS
os-unknown = Unknown

## Notifications

notif-taildrop-one = File received over Taildrop
notif-taildrop-many = { $count } files received over Taildrop
notif-save-downloads = Save to Downloads
notif-show-app = Show in Tailscale
notif-key-title = Tailscale key expiring
notif-key-expired = This machine's node key has expired. Reauthenticate to rejoin your tailnet.
notif-key-tomorrow = This machine's node key expires tomorrow. Reauthenticate to stay connected.
notif-key-days = This machine's node key expires in { $days } days. Reauthenticate to stay connected.

## Monitoring (Beszel)

page-monitoring = Monitoring

beszel-hub = Monitoring hub
beszel-unconfigured = No monitoring hub configured
beszel-unconfigured-detail = Point this at a Beszel hub on your tailnet to see hardware health beside your machines. The hub never needs to be on the public internet.
beszel-url = Hub address
beszel-url-hint = https://monitor.your-tailnet.ts.net
beszel-user = Account
beszel-password = Password
beszel-password-stored = Stored in your system keyring
beszel-connect = Connect
beszel-connecting = Connecting…
beszel-connected = Connected to { $version }
beszel-sign-out = Sign out and forget password
beszel-needs-signin = The hub rejected those credentials
beszel-unreachable = Cannot reach the hub: { $reason }
beszel-failed = { $reason }

beszel-monitored = Monitored machines
beszel-none-monitored = The hub is not monitoring any machines yet.
beszel-unmonitored = Not monitored
beszel-unmonitored-detail = These tailnet machines accept Tailscale SSH but are not reporting to the hub.
beszel-agent-version = agent { $version }
beszel-agent-outdated = agent { $version }, hub is { $hub }

beszel-cpu = CPU
beszel-memory = Memory
beszel-disk = Disk
beszel-load = Load
beszel-uptime = Up { $uptime }
beszel-temperature = Temperature
beszel-hottest = { $sensor } { $celsius }
beszel-memory-detail = { $used } of { $total } used, { $cache } reclaimable
beszel-disk-detail = { $used } of { $total }
beszel-load-detail = { $one } / { $five } / { $fifteen } across { $threads } threads
beszel-no-stats = No metrics recorded yet.
beszel-failed-services = { $count } failed services

beszel-pools = Storage pools
beszel-pool-degraded = { $pool } is { $health }
beszel-sensors = Sensors
beszel-containers = Containers
beszel-no-containers = No containers are running.
beszel-container-update = update available

## Agent deployment

beszel-install-agent = Install agent
beszel-installing = Installing…
beszel-install-title = Install the Beszel agent on { $host }?
beszel-install-explain = This runs the following command on { $host } as root, over Tailscale SSH. Read it before continuing.
beszel-install-confirm = Run as root on { $host }
beszel-install-cancel = Cancel
beszel-install-done = Agent installed. The hub will pick it up shortly.
beszel-install-failed = Agent installation failed: { $reason }
beszel-no-key = Connect to the hub first, so its public key can be read.
beszel-install-output = Installation output

beszel-alert-disk = disk { $pct }% full
beszel-alert-memory = memory { $pct }% used
beszel-alert-load = load average { $load }
beszel-alert-services = { $count } failed services
notif-hardware-title = Hardware warning

## Taildrop page

page-taildrop = Taildrop
taildrop-send = Send files
taildrop-choose-machine = Choose a machine
taildrop-selected = Selected
taildrop-no-targets = No machine on this tailnet can receive files right now.
taildrop-select-files = Select files…
taildrop-select-files-detail = Or drag them onto this window
taildrop-pick-target-first = Choose a machine above to send these to.
taildrop-received = Received files
taildrop-save-all = Save all to Downloads

## Monitoring charts

beszel-period = Showing
beszel-history-thin = Not enough history yet to draw a chart.
beszel-disk-io = Disk I/O
beszel-bandwidth = Bandwidth
beszel-read = Read
beszel-write = Write
beszel-sent = Sent
beszel-received = Received
beszel-load-1 = 1 min
beszel-load-5 = 5 min
beszel-load-15 = 15 min
beszel-temp-caption = Hottest: { $sensor } at { $celsius }°C, with { $others } other sensors shown for context
