# Installing Ken

## Server (Raspberry Pi or Linux box)

### Prerequisites

- Docker and Docker Compose installed
- A static local IP or hostname your endpoints can reach

### Steps

1. Create the configuration directory and data directory:

   ```bash
   sudo mkdir -p /etc/ken /var/lib/ken
   ```

2. Copy the example configuration:

   ```bash
   cp crates/ken-server/ken.example.toml /etc/ken/ken.toml
   ```

3. Edit `/etc/ken/ken.toml` and set `public_url` to your server's hostname or IP as seen by the agents (e.g., `https://192.168.1.10:8443`).

4. Copy and start with Docker Compose:

   ```bash
   cp crates/ken-server/docker-compose.example.yml docker-compose.yml
   docker compose up -d
   ```

5. On first startup, the server generates a bootstrap password and prints it to the logs. Retrieve it:

   ```bash
   docker compose logs ken-server | grep 'BOOTSTRAP PASSWORD'
   ```

6. Open `https://<your-server-ip>:8444/admin/login` and log in with the bootstrap password. You will be prompted to set a permanent password immediately. All subsequent logins use that password.

## Agent (Windows PC)

> **Note:** Phase 1 does not yet produce an MSI installer. The agent must be built from source with `cargo build -p ken-agent --release` targeting `x86_64-pc-windows-msvc`.

### Steps

1. Build the agent binary (on a Windows machine with Rust installed):

   ```powershell
   cargo build -p ken-agent --release --target x86_64-pc-windows-msvc
   ```

2. In the Ken admin UI, go to **Enroll** and create an enrollment URL.

3. On the Windows PC, run enrollment:

   ```powershell
   .\ken-agent.exe enroll --url <enrollment-url>
   ```

4. Install the Windows service:

   ```powershell
   .\ken-agent.exe install
   ```

5. The agent will start automatically and begin sending heartbeats.

### Verifying

Run `ken-agent.exe status` to check enrollment state and service health.

## Recovery

If you lose access to your admin password, run:

```bash
docker compose run --rm ken-server admin reset-password
```

A new bootstrap password is printed to the logs, and all active admin sessions are invalidated. Log in with the bootstrap password and set a new permanent password.
