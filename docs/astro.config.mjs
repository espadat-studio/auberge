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
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/sripwoud/auberge" }],
      customCss: ["@espadat/docs-theme/styles/theme.css"],
      components: {
        Footer: "@espadat/docs-theme/components/footer.astro",
        ThemeProvider: "@espadat/docs-theme/components/theme-provider.astro",
        ThemeSelect: "@espadat/docs-theme/components/theme-select.astro",
      },
    }),
  ],
});
