# ADR-0084: Git over SSH is the forge's own server, on the tailnet only

## Status

Accepted, 2026-10-21. Decided in #1120. Amends the `forgejo` role's former stance of "HTTPS only, no git over SSH" (the `forgejo` role README).

## Decision

**Forgejo's built-in SSH server listens on `forgejo_ssh_port` (default 2222), bound to the Host's tailnet IPv4.** `SSH_LISTEN_HOST` is that address, never a wildcard. The public address does not answer on the port because nothing listens there, and UFW's default-deny holds it closed besides.

**No firewall rule and no ACL entry are added.** UFW already allows incoming traffic on `tailscale0` (the `tailscale` role) and denies everything else. The tailnet policy already gives `tag:trusted` `*:*`. A Host without the forge therefore changes nothing: the port exists only in the `forgejo` role's `app.ini`.

**The advertised clone host is the tailnet IPv4 literal** (`forgejo_ssh_domain`, defaulting to `forgejo_tailscale_ip`). Nothing has to resolve, so the clone URL works from any tailnet client with no resolver in the path.

**The Host's admin sshd is not touched.** No change to its allowed groups, no `authorized_keys` for the `forgejo` user. `SSH_CREATE_AUTHORIZED_KEYS_FILE = false` keeps Forgejo from writing one.

**The role fails when the Host has no tailnet address**, and ends with a Readiness Probe, after a handler flush, that the port is listening on it.

## Why

The forge's public name has to stay public: Decap editors authenticate against its HTTPS. That name resolves to the public address, where the SSH port is closed. A clone URL built from it would be advertised and dead.

The built-in server keeps git's SSH identity inside Forgejo. Keys live in its database and are checked there, so the hardened admin sshd gains no second purpose and no new account.

## Trade-off

- **The clone URL names an address, not a host.** Re-enrolling the Host changes its tailnet IPv4. A redeploy rewrites `app.ini`, but remotes already cloned keep the old address.
- **`forgejo` starts only when the tailnet address exists.** The unit orders after `tailscaled.service`. A Host whose tailscale is down at boot fails the unit instead of serving HTTPS without SSH.
- **The SSH host key lives in `/var/lib/forgejo`**, which the Backup Recipe already covers. A restore without it changes the host key every client has pinned.

## Alternatives considered

- **Pass-through on the admin sshd** (a `git` user, `authorized_keys`, `ForceCommand`). Rejected: it widens the hardened sshd's allowed groups and gives the forge a login account on the Host.
- **A second DNS name through Blocky's `customDNS`.** Rejected: the `customDNS` map is built from one `subdomain:` per Playbook Meta and only for `tailnet_only` Apps. Marking the forge `tailnet_only` would unpublish its public name and break Decap. A second name needs a new publication path for one port.
- **Bind `0.0.0.0` and rely on UFW.** Rejected: one firewall reset away from a public SSH port. Binding the tailnet address makes the property structural.
- **Open the port in UFW and the ACL.** Rejected as unnecessary: both already admit tailnet traffic, and an explicit rule would exist on Hosts without the forge.
