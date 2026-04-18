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

### Prerequisites

- The Ken server must already be running and reachable from the Windows PC.
- You need administrator rights on the Windows PC.

### Steps

1. Open the [Ken releases page](https://github.com/t11z/ken/releases) and download `ken-agent.msi` from the latest release.

2. Run the installer. You can double-click the file or run it from PowerShell:

   ```powershell
   msiexec /i ken-agent.msi /quiet /norestart
   ```

   > **Self-signed publisher warning:** Until the Ken project obtains an OV or EV code-signing certificate, Windows Defender SmartScreen will show an "Unknown publisher" warning when double-clicking the MSI. Click **More info → Run anyway** to proceed. The family IT chief can also install the Ken signing certificate as trusted on each endpoint to suppress the warning; see ADR-0011 for details.

3. In the Ken admin UI, go to **Enroll** and create an enrollment URL.

4. On the Windows PC, run enrollment from an administrator PowerShell prompt:

   ```powershell
   & "C:\Program Files\Ken\ken-agent.exe" enroll --url <enrollment-url>
   ```

5. The agent service was registered by the MSI and will start automatically after enrollment, then begin sending heartbeats.

### Verifying

Run the following from an administrator PowerShell prompt to check enrollment state and service health:

```powershell
& "C:\Program Files\Ken\ken-agent.exe" status
```

## Recovery

If you lose access to your admin password, run:

```bash
docker compose run --rm ken-server admin reset-password
```

A new bootstrap password is printed to the logs, and all active admin sessions are invalidated. Log in with the bootstrap password and set a new permanent password.
