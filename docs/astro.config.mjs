import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

// The footer override reads `site` to derive the studio credit and the links
// to the sibling documentation sites. It fails the build if this is missing.
export default defineConfig({
  site: "https://auberge.espadat.com",
  integrations: [
    starlight({
      title: "auberge",
      description: "Ansible-powered VPS management without Docker bloat",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/espadat-studio/auberge" }],
      // Hand-translated from the docsify _sidebar.md, not autogenerate():
      // the groups deliberately span directories — Architecture lives under
      // core-concepts/ but belongs to Getting Started — and walking the tree
      // would throw that away.
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "Introduction", slug: "getting-started/introduction" },
            { label: "Quick Start", slug: "getting-started/quick-start" },
            { label: "Installation", slug: "getting-started/installation" },
            { label: "First Deployment", slug: "getting-started/first-deployment" },
            { label: "Second Host", slug: "getting-started/second-host" },
            { label: "Architecture", slug: "core-concepts/architecture" },
          ],
        },
        {
          label: "Configuration",
          items: [
            { label: "Hosts & Inventory", slug: "core-concepts/hosts-vs-inventory" },
            { label: "hosts.toml", slug: "configuration/hosts" },
            { label: "Host-scoped Config", slug: "configuration/host-scoped-config" },
            { label: "SSH Keys", slug: "configuration/ssh-keys" },
            { label: "Tailnet Transport", slug: "configuration/tailnet-transport" },
            { label: "Environment Variables", slug: "configuration/environment-variables" },
            { label: "Secrets", slug: "configuration/secrets" },
            { label: "Fleet GitHub Identity", slug: "configuration/fleet-github-identity" },
            { label: "Agent Tier DNS Zone", slug: "configuration/agent-tier-dns-zone" },
            { label: "Ansible Inventory", slug: "configuration/ansible-inventory" },
            { label: "Cloudflare Setup", slug: "dns/cloudflare-setup" },
          ],
        },
        {
          label: "CLI Reference",
          items: [
            { label: "Overview", slug: "cli-reference/auberge" },
            { label: "deploy", slug: "cli-reference/deploy" },
            { label: "versions", slug: "cli-reference/versions" },
            { label: "completions", slug: "cli-reference/completions" },
            {
              label: "Ansible",
              items: [
                { label: "run", slug: "cli-reference/ansible/run" },
                { label: "bootstrap", slug: "cli-reference/ansible/bootstrap" },
              ],
            },
            {
              label: "Backup",
              items: [
                { label: "create", slug: "cli-reference/backup/create" },
                { label: "list", slug: "cli-reference/backup/list" },
                { label: "restore", slug: "cli-reference/backup/restore" },
                { label: "push", slug: "cli-reference/backup/push" },
                { label: "prune", slug: "cli-reference/backup/prune" },
                { label: "sync", slug: "cli-reference/backup/sync" },
                { label: "verify", slug: "cli-reference/backup/verify" },
                { label: "export-opml", slug: "cli-reference/backup/export-opml" },
                { label: "import-opml", slug: "cli-reference/backup/import-opml" },
              ],
            },
            {
              label: "DNS",
              items: [
                { label: "list", slug: "cli-reference/dns/list" },
                { label: "status", slug: "cli-reference/dns/status" },
                { label: "set", slug: "cli-reference/dns/set" },
                { label: "set-all", slug: "cli-reference/dns/set-all" },
                { label: "migrate", slug: "cli-reference/dns/migrate" },
                { label: "delete", slug: "cli-reference/dns/delete" },
              ],
            },
            {
              label: "Host",
              items: [
                { label: "add", slug: "cli-reference/host/add" },
                { label: "list", slug: "cli-reference/host/list" },
                { label: "show", slug: "cli-reference/host/show" },
                { label: "edit", slug: "cli-reference/host/edit" },
                { label: "rename", slug: "cli-reference/host/rename" },
                { label: "remove", slug: "cli-reference/host/remove" },
                { label: "detect-tailscale-ip", slug: "cli-reference/host/detect-tailscale-ip" },
              ],
            },
            {
              label: "SSH",
              items: [
                { label: "keygen", slug: "cli-reference/ssh/keygen" },
                { label: "add-key", slug: "cli-reference/ssh/add-key" },
              ],
            },
            {
              label: "Headscale",
              items: [
                { label: "add-user", slug: "cli-reference/headscale/add-user" },
                { label: "add-key", slug: "cli-reference/headscale/add-key" },
                { label: "register", slug: "cli-reference/headscale/register" },
                { label: "tag-node", slug: "cli-reference/headscale/tag-node" },
                { label: "list-users", slug: "cli-reference/headscale/list-users" },
                { label: "list-nodes", slug: "cli-reference/headscale/list-nodes" },
                { label: "remove-user", slug: "cli-reference/headscale/remove-user" },
              ],
            },
            {
              label: "Bichon",
              items: [
                { label: "reconcile-folders", slug: "cli-reference/bichon/reconcile-folders" },
                { label: "rescan", slug: "cli-reference/bichon/rescan" },
                { label: "verify-coverage", slug: "cli-reference/bichon/verify-coverage" },
              ],
            },
            {
              label: "Sync",
              items: [
                { label: "hermes", slug: "cli-reference/sync/hermes" },
                { label: "music", slug: "cli-reference/sync/music" },
              ],
            },
            {
              label: "Config",
              items: [
                { label: "overview", slug: "cli-reference/config/overview" },
              ],
            },
            {
              label: "Select",
              items: [
                { label: "host", slug: "cli-reference/select/host" },
                { label: "playbook", slug: "cli-reference/select/playbook" },
              ],
            },
          ],
        },
        {
          label: "Backup & Restore",
          items: [
            { label: "Overview", slug: "backup-restore/overview" },
            { label: "Cross-Host Migration", slug: "backup-restore/cross-host-migration" },
            { label: "Best Practices", slug: "backup-restore/best-practices" },
            { label: "OPML Management", slug: "backup-restore/opml-management" },
          ],
        },
        {
          label: "Applications",
          items: [
            { label: "Overview", slug: "applications/overview" },
            { label: "Agent of Empires", slug: "applications/apps/aoe" },
          ],
        },
        {
          label: "Troubleshooting",
          items: [
            { label: "Common Issues", slug: "troubleshooting/common-issues" },
            { label: "SSH", slug: "troubleshooting/ssh-problems" },
            { label: "Ansible", slug: "troubleshooting/ansible-errors" },
            { label: "DNS", slug: "troubleshooting/dns-issues" },
            { label: "Backup/Restore", slug: "troubleshooting/backup-restore-issues" },
          ],
        },
        {
          label: "CI/CD",
          items: [
            { label: "Automation", slug: "deployment/ci-cd" },
          ],
        },
        {
          label: "Development",
          items: [
            { label: "Setup", slug: "development/setup" },
            { label: "Project Structure", slug: "development/project-structure" },
            { label: "Testing", slug: "development/testing" },
            { label: "Ansible Roles", slug: "development/ansible-roles" },
            { label: "Contributing", slug: "development/contributing" },
          ],
        },
        {
          label: "About",
          items: [
            { label: "Alternatives", slug: "about/alternatives" },
            { label: "License", slug: "about/license" },
          ],
        },
      ],
      customCss: ["@espadat/docs-theme/styles/theme.css"],
      components: {
        Footer: "@espadat/docs-theme/components/footer.astro",
        ThemeProvider: "@espadat/docs-theme/components/theme-provider.astro",
        ThemeSelect: "@espadat/docs-theme/components/theme-select.astro",
      },
    }),
  ],
});
