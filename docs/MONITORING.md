Because Beszel’s hub is built on PocketBase and exposes a REST API for all monitoring data, a native COSMIC extension can consume this data directly over your Tailnet without needing to load the web interface.

**Architectural Integration**

* **Direct API Polling over Tailscale:** The COSMIC applet securely connects to the Beszel Hub's IP address on the Tailnet (e.g., `[http://monitor-node.ts.net:8090](http://monitor-node.ts.net:8090)`).
* **PocketBase REST Client:** Instead of running a background agent itself, the Rust applet acts as a lightweight HTTP client, hitting Beszel’s PocketBase REST API to fetch real-time and historical metrics.
* **Node Correlation:** The extension cross-references the hostname or IP addresses from your Tailnet node list with the systems registered in Beszel, elegantly linking network status with hardware health.

**Native COSMIC Monitoring Features**

| Feature | COSMIC Implementation |
| --- | --- |
| **At-a-Glance Panel Stats** | Pin specific metrics (like a home server's CPU or memory usage) directly to the top panel next to the Tailscale network icon. |
| **Hardware Health Flyout** | Hovering over a Tailscale node in the dropdown displays immediate S.M.A.R.T. disk health, ZFS pool capacity, and sensor temperatures pulled from Beszel. |
| **Container Dashboards** | A pop-out window built in `libcosmic` displaying historical Docker and Podman statistics (CPU, memory, and network usage per container) using native charts. |
| **Desktop Notifications** | Hooking into Beszel's configurable threshold alerts, the applet can trigger native COSMIC desktop notifications when a node’s load average spikes or a disk nears capacity. |

By utilizing the encrypted Tailnet, you keep the Beszel Hub completely off the public internet while still enjoying native, system-level monitoring integration on your desktop.

---

Automating the deployment of the `beszel-agent` across a Tailnet through the COSMIC applet streamlines node management without requiring manual terminal sessions.

1. **Select Target Nodes:** Applet UI.
Select the unmonitored Tailnet machines from the applet's dropdown menu to add them to the deployment queue. Verify the selection by checking that the nodes appear in the pending deployment list.


2. **Retrieve Authentication Key:** API Request.
The applet connects to the Beszel Hub's API to fetch the required public key for agent authentication. Verify this step by checking the applet's debug log to ensure the key string is successfully loaded into memory.


3. **Execute Remote Installation:** Tailscale SSH.
The applet uses Tailscale SSH to connect and run the Beszel installation script: `curl -sL [https://get.beszel.dev](https://get.beszel.dev) -o /tmp/install-agent.sh && chmod +x /tmp/install-agent.sh && sudo /tmp/install-agent.sh -k "HUB_PUBLIC_KEY"`. The script uses root privileges to create a `beszel` user and configure a systemd service. Verify the execution by confirming a zero exit code is returned in the applet's deployment window.


4. **Verify Node Connection:** Monitoring.
The applet polls the Beszel Hub over the Tailnet to ensure the new agent is connected on the default port of 45876. Verify this by checking if the node's status indicator in the COSMIC panel turns green and displays live metrics.
