# FreshRSS Ansible Role

Deploys FreshRSS RSS reader on its own PHP-FPM pool, served by Caddy `php_fastcgi`.

## Features

- Installs FreshRSS from official GitHub repository
- Runs as dedicated system user with restricted permissions
- Uses SQLite for simple database management
- Dedicated PHP-FPM pool; Debian restarts FPM on every PHP upgrade, so the app never runs a stale binary
- Caddy `php_fastcgi` with HTTPS support
- Automatic updates via git pull

## Variables

| Variable                | Default                          | Description               |
| ----------------------- | -------------------------------- | ------------------------- |
| `freshrss_install_path` | `/opt/freshrss`                  | Installation directory    |
| `freshrss_sys_user`     | `freshrss`                       | System user               |
| `freshrss_sys_group`    | `freshrss`                       | System group              |
| `freshrss_fpm_socket`   | `/var/run/php/freshrss-fpm.sock` | PHP-FPM pool socket       |
| `freshrss_domain`       | `rss.sripwoud.xyz`               | Domain for Caddy          |
| `freshrss_version`      | `edge`                           | Git branch/tag to deploy  |
| `freshrss_data_dir`     | `/var/lib/freshrss`              | Persistent data directory |
| `freshrss_db_type`      | `sqlite`                         | Database type             |

## Post-Installation

After deployment, complete the setup via web interface:

1. Navigate to `https://{{ freshrss_domain }}`
2. Follow the installation wizard
3. Select SQLite as database
4. Complete admin account creation

## Management

```bash
# Check the PHP-FPM service
sudo systemctl status php8.4-fpm

# Update FreshRSS
cd /opt/freshrss
sudo -u freshrss git pull
sudo systemctl restart php8.4-fpm
```

## Feed Updates

Feeds are automatically updated every 15 minutes via a systemd timer (`freshrss-update.timer`).

```bash
# Check timer status
sudo systemctl status freshrss-update.timer

# View update logs
sudo journalctl -u freshrss-update.service --since "1 hour ago"
```

## Security

The pool runs as the `freshrss` user. The shared `php8.4-fpm` unit carries no per-app sandbox, unlike the retired `php -S` unit's `ProtectSystem=strict`.

## Backup

Important directories to backup:

- `/var/lib/freshrss` - User data and SQLite database
- `/opt/freshrss/data` - Configuration and cache

## Dependencies

- Caddy reverse proxy must be installed
- PHP 8.0+ with required extensions
