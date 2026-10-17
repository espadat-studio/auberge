# Stirling-PDF Ansible Role

Deploys [Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF) as a tailnet-only systemd service on a pinned Temurin JRE, behind Caddy.

## Stateless by construction

Nothing a PDF touches outlives the request. The unit's working directory and temp files sit in `/run/stirling`, a tmpfs that systemd removes when the service stops. The Playbook Meta has no `backup:` section.

Environment variables in `stirling.service.j2` turn off what would write elsewhere:

| Variable                                   | Why                                                              |
| ------------------------------------------ | ---------------------------------------------------------------- |
| `STORAGE_ENABLED=false`                    | Server-side file and signature storage                           |
| `SYSTEM_ENABLEANALYTICS=false`             | Telemetry                                                        |
| `SYSTEM_ENABLEMOBILESIGNATURE=false`       | QR relay that parks a drawn signature on the server              |
| `SYSTEM_ENABLEMOBILESCANNER=false`         | QR relay for uploads                                             |
| `PREMIUM_ENTERPRISEFEATURES_AUDIT_ENABLED` | Audit log                                                        |
| `SYSTEM_TEMPFILEMANAGEMENT_*`              | Temp files under `/run/stirling/tmp`, swept hourly as a backstop |

Verified against 3.2.0 with `/api/v1/config/app-config`: `storageEnabled`, `enableMobileSignature`, `enableMobileScanner` and `activeSecurity` all report `false`.

## Variables

| Variable                | Default                      | Description                                              |
| ----------------------- | ---------------------------- | -------------------------------------------------------- |
| `stirling_install_path` | `/opt/stirling`              | Versioned JAR directory                                  |
| `stirling_runtime_dir`  | `/run/stirling`              | tmpfs working directory                                  |
| `stirling_port`         | `8087`                       | Loopback port for Caddy                                  |
| `stirling_jvm_opts`     | `-Xmx1g ...`                 | Heap sized inside the `MemoryHigh` of the Memory Budget  |
| `stirling_jar_sha256`   | digest of `Stirling-PDF.jar` | Recomputed by `recompute-checksums.sh` on a version bump |
| `stirling_java_version` | `25.0.4.1+1`                 | Temurin JRE pin, tracked like grimmory's (ADR-0030)      |

## Not installed

LibreOffice, Tesseract, Calibre and the other converters Stirling can call. Signing and text boxes need none of them. Tools that depend on them fail until they are installed.
