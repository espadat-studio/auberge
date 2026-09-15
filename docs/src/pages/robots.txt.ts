import type { APIRoute } from "astro";

/* Generated rather than dropped in `public/`, so the host stays written down
   exactly once — in astro.config's `site`, which the footer override already
   reads. The sitemap filename is Starlight's own default. */
export const GET: APIRoute = ({ site }) =>
  new Response(`User-agent: *
Allow: /
Sitemap: ${new URL("/sitemap-index.xml", site)}
`);
