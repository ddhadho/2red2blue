The platform layer is the thinnest but most critical layer. It is the **only layer aware of the hardware**. All components above it operate in pure logic and rely on its services.

## Responsibilities

The platform layer is responsible for:

*   **Config loading and validation:** Handling the loading and validation of system configuration.
*   **Storage paths:** Managing the locations where persistent data (like WAL and `rules.toml`) is stored.
*   **Process lifecycle:** Managing the daemon's lifecycle, including starting, stopping, and signal handling.
*   **Watchdog:** Implementing a watchdog mechanism to restart the daemon if it crashes.
*   **Resource limits:** Enforcing resource limits such as memory and CPU caps.
*   **Logging output:** Directing logging output to appropriate destinations (file, stdout, syslog) based on the platform.
*   **Network interface selection:** Selecting the network interface to bind to.
